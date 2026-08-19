use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use contracts::{Storage, StorageError};

fn io_to_storage_error(key: String, err: std::io::Error) -> StorageError {
    match err.kind() {
        ErrorKind::NotFound => StorageError::NotFound(key),
        _ => StorageError::Backend(err.to_string()),
    }
}

#[derive(Clone)]
pub struct FileStorage {
    root: PathBuf,
}

impl FileStorage {
    pub fn new(work_dir: &str) -> Self {
        Self {
            root: PathBuf::from(work_dir),
        }
    }

    fn path(&self, key: &str) -> PathBuf {
        Path::new(&self.root).join(key)
    }
}

impl Storage for FileStorage {
    async fn get(&self, key: String) -> Result<String, StorageError> {
        let path = self.path(&key);

        tokio::task::spawn_blocking(move || {
            fs::read_to_string(&path).map_err(|e| io_to_storage_error(key, e))
        })
        .await
        .map_err(|err| StorageError::Backend(err.to_string()))?
    }

    async fn list(&self, key: String) -> Result<Vec<String>, StorageError> {
        let prefix = key;
        let root = self.root.clone();

        tokio::task::spawn_blocking(move || {
            // `set` создаёт `root` лениво через `create_dir_all` при первой
            // записи — до неё (свежая личность, ещё ни одного факта) каталога
            // просто нет, и это не ошибка, а пустой список.
            let entries = match fs::read_dir(&root) {
                Ok(entries) => entries,
                Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
                Err(err) => return Err(io_to_storage_error(prefix, err)),
            };

            let mut keys = Vec::new();
            for entry in entries {
                let entry = entry.map_err(|err| io_to_storage_error(prefix.clone(), err))?;
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if name.starts_with(&prefix) {
                    keys.push(name);
                }
            }
            Ok(keys)
        })
        .await
        .map_err(|err| StorageError::Backend(err.to_string()))?
    }

    async fn set(&self, key: String, content: String) -> Result<(), StorageError> {
        let root = self.root.clone();
        let path = self.path(&key);
        let tmp_path = PathBuf::from(format!("{}.tmp", path.display()));

        tokio::task::spawn_blocking(move || {
            fs::create_dir_all(&root).map_err(|err| io_to_storage_error(key.clone(), err))?;
            // Пишем во временный файл и переименовываем поверх целевого —
            // переименование в пределах одной файловой системы атомарно, так
            // что падение процесса посреди записи не оставит битый файл.
            fs::write(&tmp_path, content).map_err(|err| io_to_storage_error(key.clone(), err))?;
            fs::rename(&tmp_path, &path).map_err(|err| io_to_storage_error(key, err))?;
            Ok(())
        })
        .await
        .map_err(|err| StorageError::Backend(err.to_string()))?
    }

    async fn delete(&self, key: String) -> Result<(), StorageError> {
        let path = self.path(&key);

        tokio::task::spawn_blocking(move || {
            fs::remove_file(&path).map_err(|e| io_to_storage_error(key, e))
        })
        .await
        .map_err(|err| StorageError::Backend(err.to_string()))?
    }
}
