//! Native KWin capture. No permission-check disabling or portal impersonation.
use std::{
    os::fd::AsRawFd,
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};

#[allow(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    unused_imports,
    dead_code
)]
mod protocol {
    use wayland_client;
    use wayland_client::backend as wayland_backend;
    use wayland_client::protocol::wl_output;
    pub mod __interfaces {
        use wayland_client::backend as wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!(
            "src/overlays/screen/zkde-screencast-unstable-v1.xml"
        );
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("src/overlays/screen/zkde-screencast-unstable-v1.xml");
}
use protocol::{
    zkde_screencast_stream_unstable_v1::{self, ZkdeScreencastStreamUnstableV1},
    zkde_screencast_unstable_v1::ZkdeScreencastUnstableV1,
};

#[derive(Default)]
struct State {
    node: Option<u32>,
    failed: Option<String>,
}
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<ZkdeScreencastUnstableV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZkdeScreencastUnstableV1,
        _: protocol::zkde_screencast_unstable_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<ZkdeScreencastStreamUnstableV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZkdeScreencastStreamUnstableV1,
        event: zkde_screencast_stream_unstable_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zkde_screencast_stream_unstable_v1::Event::Created { node } => state.node = Some(node),
            zkde_screencast_stream_unstable_v1::Event::Closed => {
                state.failed = Some("KWin closed capture".into())
            }
            zkde_screencast_stream_unstable_v1::Event::Failed { error } => {
                state.failed = Some(error)
            }
            _ => {}
        }
    }
}

pub struct KdeCapture {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
    manager: ZkdeScreencastUnstableV1,
    stream: ZkdeScreencastStreamUnstableV1,
}
impl KdeCapture {
    pub fn start(uuid: &str) -> anyhow::Result<(Self, u32)> {
        let connection = Connection::connect_to_env()?;
        let (globals, queue) = registry_queue_init::<State>(&connection)?;
        let manager: ZkdeScreencastUnstableV1 = globals.bind(&queue.handle(), 1..=5, ())
            .map_err(|_| anyhow::anyhow!("KWin has not authorized native capture for this executable. Use the reviewed development desktop entry; never disable KWin permission checks."))?;
        let stream = manager.stream_window(uuid.to_owned(), 1, &queue.handle(), ());
        connection.flush()?;
        let mut capture = Self {
            connection,
            queue,
            state: State::default(),
            manager,
            stream,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            capture.pump(25)?;
            if let Some(node) = capture.state.node {
                return Ok((capture, node));
            }
        }
        anyhow::bail!("KWin capture creation timed out")
    }
    pub fn pump(&mut self, timeout_ms: i32) -> anyhow::Result<()> {
        self.queue.dispatch_pending(&mut self.state)?;
        self.connection.flush()?;
        if let Some(guard) = self.queue.prepare_read() {
            let mut fd = libc::pollfd {
                fd: guard.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // Only our Wayland connection, never an input or GPU device.
            let result = unsafe { libc::poll(&mut fd, 1, timeout_ms) };
            anyhow::ensure!(result >= 0, "Wayland poll failed");
            if result > 0 {
                guard.read()?;
            }
        }
        self.queue.dispatch_pending(&mut self.state)?;
        if let Some(error) = &self.state.failed {
            anyhow::bail!("{error}");
        }
        Ok(())
    }
}
impl Drop for KdeCapture {
    fn drop(&mut self) {
        self.stream.close();
        self.manager.destroy();
        let _ = self.connection.flush();
    }
}
