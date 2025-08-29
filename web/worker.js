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
    // Initial setup handler
    // This updates the onmessage handler immediately,
    // and only waits for init within the handler
    self.onmessage = ({data: [ptr]}) => {
        // SAT worker data handler
        self.onmessage = async ({data}) => {
            await initialized;
            let sat_worker = new SatWorker(ptr);

            sat_worker.handle_worker_msg(...data);
        }
    }
};
