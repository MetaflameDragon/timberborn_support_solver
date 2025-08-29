importScripts('main.js');

const {SatWorker} = wasm_bindgen;

// WASM init handler
self.onmessage = ({data: [mod, mem]}) => {
    // Loading WASM is async!
    let initialized = wasm_bindgen(mod, mem).catch(err => {
        // Propagate to main `onerror`
        setTimeout(() => {
            throw err;
        });
        // Rethrow to keep promise rejected
        // and prevent execution of further commands
        throw err;
    });


    console.log("Worker ready");
    // SAT worker data handler
    // This updates the onmessage handler immediately,
    // and only waits for init within the handler
    let sat_worker = null;
    self.onmessage = async ({data}) => {
        await initialized;
        if (sat_worker === null) {
            console.log("Initializing SatWorker");
            sat_worker = new SatWorker();
        }

        await sat_worker.handle_worker_msg(...data);
    }

};
