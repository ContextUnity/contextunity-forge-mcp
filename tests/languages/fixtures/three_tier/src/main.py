from helper import make


def caller(flag):
    worker = make()

    def nested(worker):
        local = worker
        return local.run()

    if flag:
        worker = make()
    worker.run()
    worker = make()
    return worker.run()


def shadow(make):
    return make()


def returned():
    return make()
