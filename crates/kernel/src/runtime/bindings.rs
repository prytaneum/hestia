wasmtime::component::bindgen!({
    path: "../sdk/wit/kernel",
    world: "host",
    imports: { "hestia:kernel/notify.confirm": async },
    exports: { default: async },
});
