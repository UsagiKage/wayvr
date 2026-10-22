// Loaded temporarily by the opt-in prototype, not installed as a KWin setting.
// UUID, destination and token are supplied as JSON literals by the caller.
const targetUuid = __UUID__;
const destination = __DESTINATION__;
const token = __TOKEN__;
let command = {};
let pending = false;
let generation = 0;
let lastGeometry = "";
const timer = new QTimer();
timer.interval = 40;
timer.timeout.connect(function () {
    if (pending) return;
    const windows = workspace.windowList();
    const target = windows.find(w => String(w.internalId) === targetUuid);
    let snapshot = { token: token, uuid: targetUuid, visible: false, active: false,
        transients: true, geometry: [0, 0, 0, 0], scale: 0, generation: generation,
        probe_seq: command.probe_seq || 0, probe: command.probe || null, hit_uuid: "" };
    if (target) {
        if (command.activate) workspace.activeWindow = target;
        command.activate = false;
        const g = target.frameGeometry;
        const geometry = [g.x, g.y, g.width, g.height];
        const scale = target.output ? target.output.scale : 0;
        const key = JSON.stringify([geometry, scale]);
        if (key !== lastGeometry) { generation++; lastGeometry = key; }
        snapshot.geometry = geometry;
        snapshot.scale = scale;
        snapshot.generation = generation;
        snapshot.active = workspace.activeWindow === target;
        snapshot.visible = !target.minimized && !target.move && !target.resize &&
            (target.onAllDesktops || target.desktops.indexOf(workspace.currentDesktop) >= 0) &&
            (target.activities.length === 0 || target.activities.indexOf(workspace.currentActivity) >= 0);
        // KWin expands capture bounds to include popups. Refuse such streams in v0.
        snapshot.transients = windows.some(w => w.transientFor === target);
        if (command.probe && command.probe.length === 2) {
            const hits = workspace.windowAt({x: command.probe[0], y: command.probe[1]}, 1);
            snapshot.hit_uuid = hits.length ? String(hits[0].internalId) : "";
        }
    }
    pending = true;
    callDBus(destination, "/org/dqi/WayVRMirror", "org.dqi.WayVRMirror", "Exchange",
        JSON.stringify(snapshot), function (reply) {
            try { command = JSON.parse(reply); }
            catch (_) { command = {}; timer.stop(); }
            pending = false;
        });
});
timer.start();
