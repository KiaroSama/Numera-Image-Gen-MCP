use crate::compat::value::{Value, json};
use crate::error::{NumeraError, Result};
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use std::{collections::HashMap, path::Path};
use uuid::Uuid;

type StoredRequestRow = (
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    i64,
    i64,
);

#[derive(Clone, Debug)]
pub struct Receipt {
    pub value: Value,
    pub generation: Option<i64>,
}

impl Receipt {
    pub fn new(value: Value) -> Self {
        Self {
            value,
            generation: None,
        }
    }
    pub fn id(&self) -> Result<&str> {
        self.value["request_id"]
            .as_str()
            .ok_or_else(NumeraError::runtime)
    }
    pub fn status(&self) -> &str {
        self.value["status"].as_str().unwrap_or("")
    }
    pub fn stamp(&mut self, generation: i64) {
        self.generation = Some(generation);
    }
    pub fn touch(&mut self) {
        self.value["updated_at"] = json!(timestamp());
    }
}

pub struct RequestRow {
    pub receipt: Receipt,
    pub identity: String,
    pub fingerprint: String,
    pub owner: Option<String>,
    pub lease: i64,
    pub generation: i64,
    pub reservation: i64,
}

pub struct Store {
    pub(super) db: Connection,
    pub owner: String,
    pub(super) generations: HashMap<String, i64>,
    lease_clock: Box<dyn Fn() -> i64 + Send + Sync>,
    pub(crate) persistence: Box<dyn super::Persistence>,
}

pub(super) fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub(super) fn busy(status: &str) -> bool {
    matches!(status, "prepared" | "submitting" | "running" | "finalizing")
}
pub(super) fn terminal(status: &str) -> bool {
    matches!(status, "completed" | "partial" | "failed" | "cancelled")
}

impl Store {
    pub const LEASE_MS: i64 = 30000;
    pub fn open(directory: &Path) -> Result<Self> {
        Self::open_with_lease_clock(directory, || Utc::now().timestamp_millis())
    }

    /// Supplies lease time only; receipt timestamps and I/O deadlines remain real-time.
    pub fn open_with_lease_clock(
        directory: &Path,
        clock: impl Fn() -> i64 + Send + Sync + 'static,
    ) -> Result<Self> {
        Self::open_with_ports(directory, clock, super::NativePersistence)
    }

    /// Keeps ordinary file I/O while allowing an instance-owned persistence implementation.
    pub fn open_with_persistence(
        directory: &Path,
        persistence: impl super::Persistence + 'static,
    ) -> Result<Self> {
        Self::open_with_ports(directory, || Utc::now().timestamp_millis(), persistence)
    }

    fn open_with_ports(
        directory: &Path,
        clock: impl Fn() -> i64 + Send + Sync + 'static,
        persistence: impl super::Persistence + 'static,
    ) -> Result<Self> {
        Ok(Self {
            db: super::migration::open(directory)?,
            owner: Uuid::new_v4().to_string(),
            generations: HashMap::new(),
            lease_clock: Box::new(clock),
            persistence: Box::new(persistence),
        })
    }

    pub(super) fn lease_now(&self) -> i64 {
        (self.lease_clock)()
    }

    pub(super) fn transaction<T>(
        &mut self,
        action: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        self.db
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|_| NumeraError::storage())?;
        match action(self) {
            Ok(value) => {
                if self.db.execute_batch("COMMIT").is_err() {
                    let _ = self.db.execute_batch("ROLLBACK");
                    return Err(NumeraError::storage());
                }
                Ok(value)
            }
            Err(error) => {
                let _ = self.db.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub(super) fn row(&self, id: &str) -> Result<RequestRow> {
        let row: Option<StoredRequestRow> = self.db.query_row(
            "SELECT receipt,identity,fingerprint,owner,lease,generation,reservation FROM requests WHERE id=?",
            [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))
            .optional().map_err(|_|NumeraError::storage())?;
        let (receipt, identity, fingerprint, owner, lease, generation, reservation) =
            row.ok_or_else(|| NumeraError::new("invalid_input", "Unknown owned request ID."))?;
        Ok(RequestRow {
            receipt: Receipt {
                value: Value::parse_with_limit(&receipt, 128 * 1024 * 1024)
                    .map_err(|_| NumeraError::storage())?,
                generation: Some(generation),
            },
            identity,
            fingerprint,
            owner,
            lease: lease.unwrap_or(0),
            generation,
            reservation,
        })
    }

    pub fn get(&self, id: &str) -> Result<Receipt> {
        Ok(self.row(id)?.receipt)
    }
    pub fn identity(&self, id: &str) -> Result<String> {
        Ok(self.row(id)?.identity)
    }
    pub fn owns(&self, id: &str, receipt: Option<&Receipt>) -> Result<bool> {
        let row = self.row(id)?;
        let generation = receipt.and_then(|r| r.generation).or_else(|| {
            if receipt.is_none() {
                self.generations.get(id).copied()
            } else {
                None
            }
        });
        Ok(row.owner.as_deref() == Some(self.owner.as_str())
            && Some(row.generation) == generation
            && row.lease > self.lease_now())
    }

    pub(super) fn fence(&self, id: &str, receipt: Option<&Receipt>) -> Result<RequestRow> {
        let row = self.row(id)?;
        if receipt.is_some() && !self.owns(id, receipt)? {
            return Err(NumeraError::new(
                "outcome_unknown",
                "Request writer generation changed; stale writes are fenced.",
            )
            .stage("storage"));
        }
        if !self.owns(id, None)? {
            return Err(NumeraError::new("outcome_unknown","Request writer lease changed; stale writes are fenced. Recover the existing request, never resubmit.").stage("storage"));
        }
        Ok(row)
    }

    pub fn prepare(
        &mut self,
        mut receipt: Receipt,
        hash: &str,
        identity: &str,
    ) -> Result<(bool, Receipt)> {
        self.transaction(|store| {
            let id = receipt.id()?.to_owned();
            let old: Option<(String,String)> = store.db.query_row("SELECT fingerprint,identity FROM requests WHERE id=?",[&id],|r|Ok((r.get(0)?,r.get(1)?)))
                .optional().map_err(|_|NumeraError::storage())?;
            if let Some((old_hash,old_identity)) = old {
                if old_hash != hash || old_identity != identity {
                    return Err(NumeraError::new("request_id_conflict","Request ID is already bound to different effective inputs or connection identity."));
                }
                store.expire(Some(&id))?;
                return Ok((false,store.get(&id)?));
            }
            store.db.execute("INSERT INTO requests(id,fingerprint,identity,receipt,owner,lease) VALUES(?,?,?,?,?,?)",
                params![id,hash,identity,receipt.value.to_string(),store.owner,store.lease_now()+Self::LEASE_MS]).map_err(|_|NumeraError::storage())?;
            store.generations.insert(id,1);
            receipt.stamp(1);
            Ok((true,receipt))
        })
    }

    pub fn update(&self, receipt: &mut Receipt) -> Result<()> {
        self.persistence.before_receipt_update()?;
        let id = receipt.id()?.to_owned();
        self.fence(&id, None)?;
        if receipt.generation != self.generations.get(&id).copied() {
            return Err(NumeraError::new(
                "outcome_unknown",
                "Receipt belongs to an earlier writer generation; stale writes are fenced.",
            )
            .stage("storage"));
        }
        receipt.touch();
        self.db.execute("UPDATE requests SET receipt=?,reservation=CASE WHEN ? THEN reservation ELSE 0 END WHERE id=? AND owner=? AND generation=?",
            params![receipt.value.to_string(),busy(receipt.status()),id,self.owner,receipt.generation]).map_err(|_|NumeraError::storage())?;
        Ok(())
    }

    pub fn renew(&self, id: &str, receipt: Option<&Receipt>) -> Result<bool> {
        let generation = receipt.and_then(|r| r.generation).or_else(|| {
            if receipt.is_none() {
                self.generations.get(id).copied()
            } else {
                None
            }
        });
        let Some(generation) = generation else {
            return Ok(false);
        };
        Ok(self
            .db
            .execute(
                "UPDATE requests SET lease=? WHERE id=? AND owner=? AND generation=? AND lease>?",
                params![
                    self.lease_now() + Self::LEASE_MS,
                    id,
                    self.owner,
                    generation,
                    self.lease_now()
                ],
            )
            .map_err(|_| NumeraError::storage())?
            == 1)
    }

    pub fn release(&mut self, id: &str, receipt: Option<&Receipt>) -> Result<()> {
        let generation = receipt.and_then(|r| r.generation).or_else(|| {
            if receipt.is_none() {
                self.generations.get(id).copied()
            } else {
                None
            }
        });
        if let Some(generation) = generation {
            self.db.execute("UPDATE requests SET owner=NULL,lease=0,reservation=CASE WHEN json_extract(receipt,'$.upstream_terminal') IS NOT NULL THEN 0 ELSE reservation END WHERE id=? AND owner=? AND generation=?",
                params![id,self.owner,generation]).map_err(|_|NumeraError::storage())?;
        }
        if self.generations.get(id).copied() == generation {
            self.generations.remove(id);
        }
        Ok(())
    }

    pub fn cancel(&self, id: &str) -> Result<()> {
        self.get(id)?;
        self.db
            .execute("UPDATE requests SET cancel=1 WHERE id=?", [id])
            .map_err(|_| NumeraError::storage())?;
        Ok(())
    }
    pub fn cancelled(&self, id: &str) -> Result<bool> {
        Ok(self
            .db
            .query_row("SELECT cancel FROM requests WHERE id=?", [id], |r| {
                r.get::<_, i64>(0)
            })
            .optional()
            .map_err(|_| NumeraError::storage())?
            == Some(1))
    }
    pub fn add_output(&self, output: &Value) -> Result<()> {
        let text = |key: &str| {
            output[key]
                .as_js_string()
                .map(|value| String::from_utf16_lossy(value.as_utf16()))
                .ok_or_else(NumeraError::storage)
        };
        self.db
            .execute(
                "INSERT INTO outputs VALUES(?,?,?)",
                params![text("output_id")?, text("request_id")?, output.to_string()],
            )
            .map_err(|_| NumeraError::storage())?;
        Ok(())
    }
    pub fn output(&self, id: &str) -> Result<Value> {
        let value: Option<String> = self
            .db
            .query_row("SELECT metadata FROM outputs WHERE id=?", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|_| NumeraError::storage())?;
        Value::parse_with_limit(
            &value.ok_or_else(|| NumeraError::new("invalid_input", "Unknown owned output ID."))?,
            128 * 1024 * 1024,
        )
        .map_err(|_| NumeraError::storage())
    }
    pub fn list_outputs(&self, offset: u32, limit: u32) -> Result<Vec<Value>> {
        let mut stmt = self
            .db
            .prepare("SELECT metadata FROM outputs ORDER BY rowid DESC LIMIT ? OFFSET ?")
            .map_err(|_| NumeraError::storage())?;
        let rows: Vec<String> = stmt
            .query_map(params![limit + 1, offset], |r| r.get(0))
            .and_then(Iterator::collect)
            .map_err(|_| NumeraError::storage())?;
        rows.into_iter()
            .map(|v| {
                Value::parse_with_limit(&v, 128 * 1024 * 1024).map_err(|_| NumeraError::storage())
            })
            .collect()
    }
    pub fn save_continuation(&self, id: &str, scope: &str, value: &Value) -> Result<()> {
        self.fence(id, None)?;
        let text = value.to_string();
        if text.len() > 1024 * 1024 {
            return Err(NumeraError::new(
                "invalid_response",
                "Continuation metadata exceeds private state limit.",
            ));
        }
        self.db.execute("INSERT INTO continuation VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET scope=excluded.scope,value=excluded.value",params![id,scope,text]).map_err(|_|NumeraError::storage())?;
        Ok(())
    }
    pub fn continuation(&self, id: &str, scope: &str) -> Result<Value> {
        let row: Option<(String, String)> = self
            .db
            .query_row(
                "SELECT scope,value FROM continuation WHERE id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|_| NumeraError::storage())?;
        match row {
            None => Ok(Value::Null),
            Some((old, value)) if old == scope => {
                Value::parse_with_limit(&value, 1024 * 1024).map_err(|_| NumeraError::storage())
            }
            _ => Err(NumeraError::new(
                "permission_denied",
                "Continuation scope does not match the selected connection/account/model.",
            )),
        }
    }
}
