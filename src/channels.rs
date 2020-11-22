#![allow(unused)]

use std::{
    future::Future,
    ops::Deref,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    time::Duration,
};

use bevy::tasks::{Task, TaskPool};
use futures::{stream, Stream};

use turbulence::{
    buffer::BufferPool,
    packet::{Packet, PacketPool},
    packet_multiplexer::{MuxPacket, MuxPacketPool},
    runtime::Runtime,
};

#[derive(Clone, Debug)]
pub struct SimpleBufferPool(pub usize);

impl BufferPool for SimpleBufferPool {
    type Buffer = Box<[u8]>;

    fn acquire(&self) -> Self::Buffer {
        vec![0; self.0].into_boxed_slice()
    }
}

#[derive(Clone)]
pub struct TaskPoolRuntime(Arc<TaskPoolRuntimeInner>);

pub struct TaskPoolRuntimeInner {
    pool: TaskPool,
    tasks: Mutex<Vec<Task<()>>>, // FIXME: cleanup finished
}

impl TaskPoolRuntime {
    pub fn new(pool: TaskPool) -> Self {
        TaskPoolRuntime(Arc::new(TaskPoolRuntimeInner {
            pool,
            tasks: Mutex::new(Vec::new()),
        }))
    }
}

impl Deref for TaskPoolRuntime {
    type Target = TaskPoolRuntimeInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Runtime for TaskPoolRuntime {
    type Instant = instant::Instant;
    type Sleep = Pin<Box<dyn Future<Output = ()> + Send>>;

    fn spawn<F: Future<Output = ()> + Send + 'static>(&self, f: F) {
        self.tasks
            .lock()
            .unwrap()
            .push(self.pool.spawn(Box::pin(f)));
    }

    fn now(&self) -> Self::Instant {
        Self::Instant::now()
    }

    fn elapsed(&self, instant: Self::Instant) -> Duration {
        instant.elapsed()
    }

    fn duration_between(&self, earlier: Self::Instant, later: Self::Instant) -> Duration {
        later.duration_since(earlier)
    }

    fn sleep(&self, duration: Duration) -> Self::Sleep {
        let state = Arc::clone(&self.0);
        Box::pin(async move {
            do_delay(state, duration).await;
        })
    }
}

async fn do_delay(state: Arc<TaskPoolRuntimeInner>, duration: Duration) -> instant::Instant {
    state
        .pool
        .spawn(async move {
            std::thread::sleep(duration);
        })
        .await;
    instant::Instant::now()
}
