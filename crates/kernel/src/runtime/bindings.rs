wasmtime::component::bindgen!({
    path: "../sdk/wit/kernel",
    world: "host",
    exports: { default: async },
});
