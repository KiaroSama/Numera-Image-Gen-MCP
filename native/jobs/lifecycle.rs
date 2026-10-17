use super::store::{Receipt, Store, busy, terminal};
use crate::compat::value::{Value, json};
use crate::error::{NumeraError, Result};
use rusqlite::params;

impl Store {
    pub(super) fn expire(&mut self, id: Option<&str>) -> Result<()> {
        self.db.execute_batch("UPDATE requests SET reservation=0 WHERE owner IS NULL AND json_extract(receipt,'$.status')='outcome_unknown' AND json_extract(receipt,'$.upstream_job') IS NULL AND NOT EXISTS(SELECT 1 FROM result_journal WHERE request_id=requests.id AND phase!='committed')")
            .map_err(|_|NumeraError::storage())?;
        let ids: Vec<String> = {
            let mut query=self.db.prepare("SELECT id FROM requests WHERE lease<=? AND owner IS NOT NULL AND (? IS NULL OR id=?)").map_err(|_|NumeraError::storage())?;
            query
                .query_map(params![self.lease_now(), id, id], |r| r.get(0))
                .and_then(Iterator::collect)
                .map_err(|_| NumeraError::storage())?
        };
        for id in ids {
            let row = self.row(&id)?;
            let mut receipt = row.receipt;
            let retained = self.has_pending(&id)?;
            if busy(receipt.status()) && receipt.value["upstream_terminal"].is_null() {
                if receipt.status() == "prepared" {
                    receipt.value["status"] = json!("failed");
                    receipt.value["generation_outcome"] = json!("not_submitted");
                } else if !retained
                    && receipt.value["outputs"]
                        .as_array()
                        .is_none_or(Vec::is_empty)
                {
                    let upstream = !receipt.value["upstream_job"].is_null();
                    receipt.value["status"] = json!(if upstream {
                        "running"
                    } else {
                        "outcome_unknown"
                    });
                    receipt.value["generation_outcome"] =
                        json!(if upstream { "running" } else { "unknown" });
                }
                receipt.touch();
            }
            let reserve =
                retained || (!receipt.value["upstream_job"].is_null() && busy(receipt.status()));
            self.db.execute("UPDATE requests SET receipt=?,owner=NULL,lease=0,generation=generation+1,reservation=CASE WHEN ? THEN reservation ELSE 0 END WHERE id=? AND generation=? AND lease<=?",
                params![receipt.value.to_string(),reserve,id,row.generation,self.lease_now()]).map_err(|_|NumeraError::storage())?;
        }
        Ok(())
    }

    pub fn terminate(
        &mut self,
        id: &str,
        state: &str,
        error: &Value,
        expected: Option<&Receipt>,
        job: Option<&Value>,
    ) -> Result<Receipt> {
        if !matches!(state, "failed" | "cancelled" | "incomplete") {
            return Err(NumeraError::new(
                "invalid_response",
                "Invalid terminal provider state.",
            ));
        }
        self.transaction(|store| {
            if expected.is_some() {store.fence(id,expected)?;}
            let row=store.row(id)?;
            let mut receipt=row.receipt;
            if let Some(job)=job
                && (receipt.value["upstream_job"]["id"]!=job["id"] || receipt.value["upstream_job"]["kind"]!=job["kind"]) {
                return Err(NumeraError::new("permission_denied","Terminal result does not match the existing job."));
            }
            if !receipt.value["upstream_terminal"].is_null() || receipt.value["generation_outcome"]=="completed" { return Ok(()); }
            receipt.value["upstream_terminal"]=json!(state);
            receipt.value["generation_outcome"]=json!(state);
            let output=receipt.value["outputs"].as_array().is_some_and(|v|!v.is_empty());
            receipt.value["status"]=json!(if output {"partial"} else if state=="cancelled" {"cancelled"} else {"failed"});
            receipt.value["errors"].as_array_mut().ok_or_else(NumeraError::storage)?.push(error.clone());
            receipt.touch();
            let keep=expected.is_some();
            store.db.execute("UPDATE requests SET receipt=?,reservation=CASE WHEN ? THEN reservation ELSE 0 END,owner=CASE WHEN ? THEN owner ELSE NULL END,lease=CASE WHEN ? THEN lease ELSE 0 END,generation=generation+CASE WHEN ? THEN 0 ELSE 1 END WHERE id=? AND generation=?",
                params![receipt.value.to_string(),keep,keep,keep,keep,id,row.generation]).map_err(|_|NumeraError::storage())?;
            Ok(())
        })?;
        self.get(id)
    }

    pub fn admit(
        &mut self,
        id: &str,
        connection: &str,
        global: usize,
        per_connection: usize,
    ) -> Result<bool> {
        self.transaction(|store| {
            store.expire(None)?;
            let values:Vec<String>={
                let mut stmt=store.db.prepare("SELECT receipt FROM requests WHERE lease>? OR (owner IS NULL AND json_extract(receipt,'$.status')='running' AND json_extract(receipt,'$.upstream_job') IS NOT NULL)").map_err(|_|NumeraError::storage())?;
                stmt.query_map([store.lease_now()],|r|r.get(0)).and_then(Iterator::collect).map_err(|_|NumeraError::storage())?
            };
            let all:Vec<Value>=values.into_iter().map(|v|Value::parse_with_limit(&v,128*1024*1024).map_err(|_|NumeraError::storage())).collect::<Result<Vec<Value>>>()?
                .into_iter().filter(|v|matches!(v["status"].as_str(),Some("submitting"|"running"|"finalizing")) && v["upstream_terminal"].is_null() && v["generation_outcome"]!="completed").collect();
            if all.len()>=global || all.iter().filter(|v|v["connection"]==connection).count()>=per_connection {return Ok(false);}
            let mut receipt=store.get(id)?;
            if receipt.status()!="prepared" || !store.owns(id,None)? {return Ok(false);}
            receipt.value["status"]=json!("submitting");
            store.update(&mut receipt)?;
            Ok(true)
        })
    }

    pub fn claim(&mut self, id: &str, expected: Option<&Receipt>, received: bool) -> Result<bool> {
        self.transaction(|store| {
            store.expire(Some(id))?;
            let row=store.row(id)?;
            let mut receipt=row.receipt;
            if expected.is_some() && !store.owns(id,expected)? {return Ok(false);}
            if row.owner.is_some() && !store.owns(id,None)? {return Ok(false);}
            if receipt.status()=="finalizing" && row.owner.is_some() {return Ok(false);}
            if row.owner.is_none() && receipt.status()=="prepared" {return Ok(false);}
            if receipt.status()=="submitting" && row.owner.is_some() && expected.is_none() && !store.has_pending(id)? && !received {return Ok(false);}
            if terminal(receipt.status()) && !store.has_pending(id)? && !received {return Ok(false);}
            let generation=row.generation+1;
            receipt.value["status"]=json!("finalizing");receipt.touch();
            store.db.execute("UPDATE requests SET receipt=?,owner=?,generation=?,lease=? WHERE id=? AND generation=?",
                params![receipt.value.to_string(),store.owner,generation,store.lease_now()+Self::LEASE_MS,id,row.generation]).map_err(|_|NumeraError::storage())?;
            store.generations.insert(id.to_owned(),generation);
            Ok(true)
        })
    }

    pub fn queued(&mut self) -> Result<usize> {
        self.transaction(|store| {
            store.expire(None)?;
            store.db.query_row("SELECT COUNT(*) FROM requests WHERE json_extract(receipt,'$.status')='prepared'",[],|r|r.get::<_,i64>(0))
                .map(|v|v as usize).map_err(|_|NumeraError::storage())
        })
    }
}
