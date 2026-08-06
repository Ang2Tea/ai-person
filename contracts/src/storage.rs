use crate::StorageError;

pub trait Storage {
    fn get(&self, key: String) -> impl Future<Output = Result<String, StorageError>> + Send;
    fn list(&self, key: String) -> impl Future<Output = Result<Vec<String>, StorageError>> + Send;
    fn set(
        &self,
        key: String,
        content: String,
    ) -> impl Future<Output = Result<(), StorageError>> + Send;
    fn delete(&self, key: String) -> impl Future<Output = Result<(), StorageError>> + Send;
}
