mod helper;
use helper::make;

pub fn caller(flag: bool) -> i32 {
    let mut worker = make();
    fn nested() -> i32 {
        7
    }
    if flag {
        worker = make();
    }
    let scalar = {
        let worker = 1;
        worker
    };
    let _ = scalar;
    worker.run();
    worker = make();
    worker.run() + nested()
}

pub fn shadow(make: fn() -> i32) -> i32 {
    make()
}

pub fn typed_receiver(worker: helper::Worker) -> i32 {
    worker.run()
}
