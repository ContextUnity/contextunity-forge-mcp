import { make, Worker } from "./helper";

export function caller(flag: boolean) {
    let worker = make();
    function nested(worker: Worker) {
        const alias = worker;
        return alias.run();
    }
    if (flag) {
        worker = make();
    }
    worker.run();
    worker = make();
    try {
        return worker.run() + nested(worker);
    } finally {
        const marker = 1;
    }
}

export function shadow(make: () => number) {
    return make();
}

export function returned() {
    return make();
}
