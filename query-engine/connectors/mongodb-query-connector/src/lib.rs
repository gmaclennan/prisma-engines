#![allow(clippy::clippy::vec_init_then_push)]

mod cursor;
mod error;
mod filter;
mod interface;
mod join;
mod orderby;
mod output_meta;
mod projection;
mod query_builder;
mod root_queries;
mod value;

use error::MongoError;
use futures::stream::StreamExt;
use mongodb::{
    bson::{Bson, Document},
    Cursor,
};

pub use interface::*;

type Result<T> = std::result::Result<T, MongoError>;

trait IntoBson {
    fn into_bson(self) -> Result<Bson>;
}

trait BsonTransform {
    fn into_document(self) -> Result<Document>;
}

impl BsonTransform for Bson {
    fn into_document(self) -> Result<Document> {
        if let Bson::Document(doc) = self {
            Ok(doc)
        } else {
            Err(MongoError::ConversionError {
                from: format!("{:?}", self),
                to: "Bson::Document".to_string(),
            })
        }
    }
}

// Todo: Move to approriate place
/// Consumes a cursor stream until exhausted.
async fn vacuum_cursor(mut cursor: Cursor<Document>) -> crate::Result<Vec<Document>> {
    let mut docs = vec![];

    while let Some(result) = cursor.next().await {
        match result {
            Ok(document) => docs.push(document),
            Err(e) => return Err(e.into()),
        }
    }

    Ok(docs)
}
