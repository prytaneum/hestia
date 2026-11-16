wasmtime::component::bindgen!({
    path: "../sdk/wit/kernel",
    world: "host",
    imports: {
        "hestia:kernel/notify.confirm": async,
        "hestia:kernel/download.fetch": async,
        "hestia:kernel/programs.run": async,
        "hestia:kernel/ipc.request": async,
    },
    exports: { default: async },
});
