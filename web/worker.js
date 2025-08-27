importScripts('main.js');

const {SatWorker} = wasm_bindgen;

self.onmessage = event => {
    // Loading WASM is async!
    let initialized = wasm_bindgen(...event.data).catch(err => {
        // Propagate to main `onerror`
        setTimeout(() => {
            throw err;
        });
        // Rethrow to keep promise rejected
        // and prevent execution of further commands
        throw err;
    });

    let sat_worker = null;

    console.log("Worker ready");
    // This updates the onmessage handler immediately,
    // and only waits for init within the handler
    self.onmessage = async event => {
        await initialized;
        if (sat_worker === null) {
            sat_worker = new SatWorker();
        }
        sat_worker.handle_worker_msg(...event.data);
    }
};
