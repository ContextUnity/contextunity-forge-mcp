pub struct Worker;

impl Worker {
    pub fn run(&self) -> i32 {
        3
    }
}

pub fn make() -> Worker {
    Worker
}
