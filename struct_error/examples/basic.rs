use struct_error::{error, match_error, throw, throws, united_error};

#[error("resource not found: {}", self.id)]
pub struct NotFound {
    pub id: u64,
}

#[error("connection timed out after {}ms", self.ms)]
pub struct Timeout {
    pub ms: u64,
}

#[error("I/O error")]
pub struct IoError {
    #[error_source]
    pub inner: std::io::Error,
}

#[united_error(NotFound, Timeout)]
pub struct AppError;

#[throws(NotFound, Timeout)]
pub fn fetch_resource(id: u64) -> String {
    if id == 0 {
        throw!(NotFound { id });
    }
    if id > 100 {
        throw!(Timeout { ms: 5000 });
    }
    format!("resource-{}", id)
}

#[throws(NotFound, Timeout)]
pub fn process(id: u64) -> String {
    let res = fetch_resource(id)?;
    res.to_uppercase()
}

#[throws(AppError)]
pub fn process_united(id: u64) -> String {
    let res = fetch_resource(id)?;
    res.to_uppercase()
}

fn main() {
    for id in [0, 42, 101] {
        let result = process(id);
        match_error!(result {
            Ok(v) => println!("[{}] success: {}", id, v),
            NotFound { id } => println!("[{}] not found: {}", id, id),
            Timeout { ms } => println!("[{}] timeout: {}ms", id, ms),
        });
    }

    println!("--- united error ---");
    match_error!(process_united(0) {
        Ok(v) => println!("success: {}", v),
        NotFound { .. } => println!("not found"),
        Timeout { .. } => println!("timeout"),
    });
}
