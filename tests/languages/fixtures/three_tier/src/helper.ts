export class Worker {
    run(): number {
        return 3;
    }
}

export function make() {
    return new Worker();
}
