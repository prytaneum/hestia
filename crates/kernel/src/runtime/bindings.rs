wasmtime::component::bindgen!({
    path: "../sdk/wit/kernel",
    world: "host",
    imports: {
        "hestia:kernel/notify.confirm": async,
        "hestia:kernel/download.fetch": async,
        "hestia:kernel/settings.get": async,
        "hestia:kernel/settings.set": async,
        "hestia:kernel/secrets.get": async,
        "hestia:kernel/secrets.set": async,
        "hestia:kernel/programs.run": async,
        "hestia:kernel/ipc.connect": async,
        "hestia:kernel/ipc.[method]connection.write": async,
        "hestia:kernel/ipc.[method]connection.read": async,
    },
    with: {
        "hestia:kernel/ipc.connection": super::ipc::Connection,
    },
    exports: { default: async },
});
