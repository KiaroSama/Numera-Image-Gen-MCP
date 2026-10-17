use super::store::{Receipt, Store, terminal};
use crate::compat::hash::sha256;
use crate::compat::value::{JsString, Value, json};
use crate::error::{NumeraError, Result};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct ResultItem {
    pub index: u32,
    pub output_id: String,
    pub target: Option<String>,
    pub bytes: Option<Vec<u8>>,
    pub url: Option<JsString>,
    pub sha256: Option<String>,
    pub phase: String,
}

fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<ResultItem> {
    Ok(ResultItem {
        index: row.get(0)?,
        output_id: row.get(1)?,
        target: row.get(2)?,
        bytes: row.get(3)?,
        url: row.get::<_, Option<String>>(4)?.map(JsString::from),
        sha256: row.get(5)?,
        phase: row.get(6)?,
    })
}

impl Store {
    fn item(&self, id: &str, index: u32) -> Result<Option<ResultItem>> {
        if index >= 10 {
            return Err(NumeraError::new(
                "invalid_response",
                "Provider returned too many final image items.",
            )
            .stage("response"));
        }
        self.db.query_row("SELECT item,output_id,target,bytes,url,sha256,phase FROM result_journal WHERE request_id=? AND item=?",params![id,index],decode)
            .optional().map_err(|_|NumeraError::storage())
    }
    pub fn has_pending(&self, id: &str) -> Result<bool> {
        self.db.query_row("SELECT EXISTS(SELECT 1 FROM result_journal WHERE request_id=? AND phase!='committed')",[id],|r|r.get(0)).map_err(|_|NumeraError::storage())
    }
    pub fn needs_local_recovery(&self, id: &str) -> Result<bool> {
        let receipt = self.get(id)?;
        Ok(self.has_pending(id)?
            || (receipt.value["generation_outcome"] == "completed"
                && receipt.value["outputs"]
                    .as_array()
                    .is_some_and(|v| !v.is_empty())
                && !terminal(receipt.status())))
    }
    pub fn pending(&self, id: &str) -> Result<Vec<ResultItem>> {
        let mut stmt=self.db.prepare("SELECT item,output_id,target,bytes,url,sha256,phase FROM result_journal WHERE request_id=? AND phase!='committed' ORDER BY item").map_err(|_|NumeraError::storage())?;
        stmt.query_map([id], decode)
            .and_then(Iterator::collect)
            .map_err(|_| NumeraError::storage())
    }
    pub fn reserve(&mut self, id: &str, bytes: i64, budget: i64) -> Result<()> {
        if bytes <= 0 || budget <= 0 || bytes > 9007199254740991 || budget > 9007199254740991 {
            return Err(NumeraError::new(
                "invalid_configuration",
                "Result journal limits must be positive safe integers.",
            ));
        }
        self.transaction(|store| {
            store.fence(id,None)?;
            let used:i64=store.db.query_row("SELECT COALESCE(SUM(MAX(reservation,COALESCE((SELECT SUM(COALESCE(length(bytes),0)+COALESCE(length(CAST(url AS BLOB)),0)) FROM result_journal WHERE request_id=requests.id),0))),0) FROM requests WHERE id!=?",[id],|r|r.get(0)).map_err(|_|NumeraError::storage())?;
            let retained:i64=store.db.query_row("SELECT COALESCE(SUM(COALESCE(length(bytes),0)+COALESCE(length(CAST(url AS BLOB)),0)),0) FROM result_journal WHERE request_id=?",[id],|r|r.get(0)).map_err(|_|NumeraError::storage())?;
            let reserve=bytes.max(retained);
            if used.checked_add(reserve).is_none_or(|v|v>budget) {
                return Err(NumeraError::new("rate_limited","Private result journal capacity is full; recover retained outputs before authorizing more image work.").stage("storage"));
            }
            store.db.execute("UPDATE requests SET reservation=? WHERE id=?",params![reserve,id]).map_err(|_|NumeraError::storage())?;
            Ok(())
        })
    }
    fn other_bytes(&self, id: &str, index: u32) -> Result<i64> {
        self.db.query_row("SELECT COALESCE(SUM(COALESCE(length(bytes),(SELECT json_extract(metadata,'$.bytes') FROM outputs WHERE id=output_id),0)+COALESCE(length(CAST(url AS BLOB)),0)),0) FROM result_journal WHERE request_id=? AND item!=?",params![id,index],|r|r.get(0)).map_err(|_|NumeraError::storage())
    }
    pub fn retain(
        &mut self,
        id: &str,
        index: u32,
        bytes: &[u8],
        expected: Option<&Receipt>,
    ) -> Result<()> {
        self.transaction(|store| {
            let request=store.fence(id,expected)?;
            let old=store.item(id,index)?;
            let hash=sha256(bytes);
            if old.as_ref().and_then(|v|v.sha256.as_deref()).is_some_and(|v|v!=hash) {
                return Err(NumeraError::new("invalid_response","An existing result item changed its image bytes.").stage("response"));
            }
            if old.as_ref().is_some_and(|v|v.phase=="committed") {return Ok(());}
            let total=store.other_bytes(id,index)?.checked_add(bytes.len() as i64);
            if bytes.is_empty() || total.is_none_or(|v|v>request.reservation) {
                return Err(NumeraError::new("invalid_response","Aggregate output bytes exceeded.").stage("response"));
            }
            let output_id=old.map_or_else(||Uuid::new_v4().to_string(),|v|v.output_id);
            store.db.execute("INSERT INTO result_journal(request_id,item,output_id,bytes,sha256,phase) VALUES(?,?,?,?,?,'retained') ON CONFLICT(request_id,item) DO UPDATE SET bytes=excluded.bytes,url=NULL,sha256=excluded.sha256,phase=CASE WHEN result_journal.target IS NULL THEN 'retained' ELSE 'publishing' END",params![id,index,output_id,bytes,hash]).map_err(|_|NumeraError::storage())?;
            let mut receipt=store.get(id)?;
            receipt.value["generation_outcome"]=if receipt.value["upstream_terminal"].is_null(){json!("completed")}else{receipt.value["upstream_terminal"].clone()};
            receipt.touch();
            // Retention cannot release capacity while the terminal writer still owns it.
            store.db.execute("UPDATE requests SET receipt=? WHERE id=? AND owner=? AND generation=?",
                params![receipt.value.to_string(),id,store.owner,request.generation]).map_err(|_|NumeraError::storage())?;
            Ok(())
        })
    }
    pub fn retain_url(
        &mut self,
        id: &str,
        index: u32,
        url: &JsString,
        expected: Option<&Receipt>,
    ) -> Result<()> {
        // Node SQLite binds JS strings as UTF-8, replacing unpaired surrogates at this
        // existing TEXT-column boundary. Do not add a marker or change schema2 storage.
        let url = String::from_utf16_lossy(url.as_utf16());
        self.transaction(|store| {
            let row=store.fence(id,expected)?;
            let old=store.item(id,index)?;
            if old.as_ref().is_some_and(|v|v.bytes.is_some()||v.phase=="committed") {return Ok(());}
            if url.len()>16384 {return Err(NumeraError::new("invalid_response","Image continuation URL exceeds private state limit."));}
            if store.other_bytes(id,index)?.checked_add(url.len() as i64).is_none_or(|v|v>row.reservation) {
                return Err(NumeraError::new("invalid_response","Aggregate output bytes exceeded.").stage("response"));
            }
            let output_id=old.map_or_else(||Uuid::new_v4().to_string(),|v|v.output_id);
            store.db.execute("INSERT INTO result_journal(request_id,item,output_id,url,phase) VALUES(?,?,?,?,'waiting') ON CONFLICT(request_id,item) DO UPDATE SET url=excluded.url",params![id,index,output_id,url]).map_err(|_|NumeraError::storage())?;
            Ok(())
        })
    }
    pub fn publication(
        &mut self,
        id: &str,
        index: u32,
        target: impl FnOnce(&str) -> Result<String>,
        expected: Option<&Receipt>,
    ) -> Result<ResultItem> {
        self.transaction(|store| {
            store.fence(id,expected)?;
            let mut item=store.item(id,index)?.ok_or_else(||NumeraError::new("output_file_error","Stable publication requires retained, validated image bytes.").stage("storage"))?;
            if item.sha256.is_none() {return Err(NumeraError::new("output_file_error","Stable publication requires retained, validated image bytes.").stage("storage"));}
            let path=match &item.target {Some(path)=>path.clone(),None=>target(&item.output_id)?};
            store.db.execute("UPDATE result_journal SET target=?,phase=CASE WHEN phase='committed' THEN phase ELSE 'publishing' END WHERE request_id=? AND item=?",params![path,id,index]).map_err(|_|NumeraError::storage())?;
            item.target=Some(path);item.url=None;
            Ok(item)
        })
    }
    pub fn commit(
        &mut self,
        id: &str,
        index: u32,
        output: &Value,
        deviations: &[String],
        expected: Option<&Receipt>,
    ) -> Result<()> {
        self.transaction(|store| {
            store.fence(id,expected)?;
            let item=store.item(id,index)?.ok_or_else(NumeraError::storage)?;
            if Some(item.output_id.as_str())!=output["output_id"].as_str() || item.sha256.as_deref()!=output["sha256"].as_str() || item.target.as_deref()!=output["path"].as_str() {
                return Err(NumeraError::new("output_file_error","Published output does not match its retained journal identity.").stage("storage"));
            }
            if item.phase=="committed" {return Ok(());}
            store.add_output(output)?;
            let mut receipt=store.get(id)?;
            receipt.value["outputs"].as_array_mut().ok_or_else(NumeraError::storage)?.push(output.clone());
            receipt.value["deviations"].as_array_mut().ok_or_else(NumeraError::storage)?.extend(deviations.iter().map(|s|json!(s)));
            store.update(&mut receipt)?;
            store.db.execute("UPDATE result_journal SET bytes=NULL,url=NULL,phase='committed' WHERE request_id=? AND item=?",params![id,index]).map_err(|_|NumeraError::storage())?;
            Ok(())
        })
    }
}
