#![allow(dead_code)]
//! The goal of Simulation is to provide a set of low level components which can be
//! used to write applications amenable to [FoundationDB style simulation testing](https://apple.github.io/foundationdb/testing.html).
//!
//! Simulation is an abstraction over [Tokio], allowing application developers to write
//! applications which are generic over sources of nondeterminism. Additionally, Simulation
//! provides deterministic analogues to time, scheduling, network and IO.
//!
//! # Scheduling and Time
//!
//! Simulation provides a mock source of time. Mock time will only advance when the executor
//! has no more work to do. This can be used to force deterministic reordering of task execution.
//!
//! When time is advanced, it is advanced instantly to a value which allows the executor to make
//! progress. Applications which rely on timeouts can then be tested in a fraction of the time it
//! would normally take to test a particular execution ordering.
//!
//! # Network
//!
//! Simulation includes an in-memory network. Applications can use `Environment::bind` and `Environment::connect`
//! to create in-memory connections between components. The in-memory connections will automatically have delays
//! and disconnect faults injected, dependent on an initial seed value.
//!
//! # Faults
//!
//! Faults are injected based on a seedable RNG, causing IO delays and disconnects.
//! This is sufficient to trigger bugs in higher level components, such as message reordering.
//!
//! By eliminating sources of nondeterminism, and basing fault injection on a seedable RNG, it's
//! possible to run many thousands of tests in the span of a few seconds with different fault
//! injections. This allows testing different execution orderings. If a particular seed causes a
//! failing execution ordering, developers can use the seed value to debug and fix their applications.
//!
//! Once the error is fixed, the seed value can be used to setup a regression test to ensure that the
//! issue stays fixed.
//!
//! # Example
//!
//! ```rust
//!    use simulation::{Environment, TcpListener};
//!    use futures::{SinkExt, StreamExt};
//!    use std::{io, net, time};
//!    use tokio::codec::{Framed, LinesCodec};
//!
//!    /// Start a client request handler which will write greetings to clients.
//!    async fn handle<E>(env: E, socket: <E::TcpListener as TcpListener>::Stream, addr: net::SocketAddr)
//!    where
//!        E: Environment,
//!    {
//!        // delay the response, in deterministic mode this will immediately progress time.
//!        env.delay_from(time::Duration::from_secs(1));
//!        println!("handling connection from {:?}", addr);
//!        let mut transport = Framed::new(socket, LinesCodec::new());
//!        if let Err(e) = transport.send(String::from("Hello World!")).await {
//!            println!("failed to send response: {:?}", e);
//!        }
//!    }
//!
//!    /// Start a server which will bind to the provided addr and repyl to clients.
//!    async fn server<E>(env: E, addr: net::SocketAddr) -> Result<(), io::Error>
//!    where
//!        E: Environment,
//!    {
//!        let mut listener = env.bind(addr).await?;
//!
//!        while let Ok((socket, addr)) = listener.accept().await {
//!            let request = handle(env.clone(), socket, addr);
//!            env.spawn(request)
//!        }
//!        Ok(())
//!    }
//!
//!
//!    /// Create a client which will read a message from the server
//!    async fn client<E>(env: E, addr: net::SocketAddr) -> Result<(), io::Error>
//!    where
//!        E: Environment,
//!    {
//!        loop {
//!            match env.connect(addr).await {
//!                Err(_) => {
//!                    // Sleep if the connection was rejected, retrying later.
//!                    // In deterministic mode, this will just reorder task execution
//!                    // without waiting for time to advance.
//!                    env.delay_from(time::Duration::from_secs(1)).await;
//!                    continue;
//!                }
//!                Ok(conn) => {
//!                    let mut transport = Framed::new(conn, LinesCodec::new());
//!                    let result = transport.next().await.unwrap().unwrap();
//!                    assert_eq!(result, "Hello World!");
//!                    return Ok(());
//!                }
//!            }
//!        }
//!    }
//!    #[test]
//!    fn test() {
//!        // This particular example will fail with a seed value of 22 due to not handling disconnects.
//!        let mut runtime = simulation::deterministic::DeterministicRuntime::new_with_seed(1).unwrap();
//!        let handle = runtime.handle();
//!        runtime.block_on(async {
//!            let bind_addr: net::SocketAddr = "127.0.0.1:8080".parse().unwrap();
//!            let server = server(handle.clone(), bind_addr);
//!            handle.spawn(async move {
//!                server.await.unwrap();
//!            });
//!            client(handle, bind_addr).await.unwrap();
//!        })
//!    }
//! ```
//!
//! [Tokio]: https://github.com/tokio-rs
//! [CurrentThread]:[tokio_executor::current_thread::CurrentThread]
//! [Delay]:[tokio_timer::Delay]
//! [Timeout]:[tokio_timer::Timeout]

use async_trait::async_trait;
use futures::{Future, FutureExt};
use std::{io, net, time};
use tokio::io::{AsyncRead, AsyncWrite};

pub mod deterministic;
pub mod singlethread;

mod example {
    use crate::{Environment, TcpListener};
    use futures::{SinkExt, StreamExt};
    use std::{io, net, time};
    use tokio::codec::{Framed, LinesCodec};

    /// Start a client request handler which will write greetings to clients.
    async fn handle<E>(
        env: E,
        socket: <E::TcpListener as TcpListener>::Stream,
        addr: net::SocketAddr,
    ) where
        E: Environment,
    {
        // delay the response, in deterministic mode this will immediately progress time.
        env.delay_from(time::Duration::from_secs(1));
        println!("handling connection from {:?}", addr);
        let mut transport = Framed::new(socket, LinesCodec::new());
        if let Err(e) = transport.send(String::from("Hello World!")).await {
            println!("failed to send response: {:?}", e);
        }
    }

    /// Start a server which will bind to the provided addr and repyl to clients.
    async fn server<E>(env: E, addr: net::SocketAddr) -> Result<(), io::Error>
    where
        E: Environment,
    {
        let mut listener = env.bind(addr).await?;

        while let Ok((socket, addr)) = listener.accept().await {
            let request = handle(env.clone(), socket, addr);
            env.spawn(request)
        }
        Ok(())
    }

    /// Create a client which will read a message from the server
    async fn client<E>(env: E, addr: net::SocketAddr) -> Result<(), io::Error>
    where
        E: Environment,
    {
        let conn = env.connect(addr).await?;
        let mut transport = Framed::new(conn, LinesCodec::new());
        let result = transport.next().await.unwrap().unwrap();
        assert_eq!(result, "Hello world!");
        Ok(())
    }

    fn main() {
        // Runtimes, networking and fault injection can be swapped out.
        // let mut runtime = crate::singlethread::SingleThreadedRuntime::new().unwrap();
        let mut runtime = crate::deterministic::DeterministicRuntime::new().unwrap();
        let handle = runtime.handle();
        let addr: net::SocketAddr = "127.0.0.1:9090".parse().unwrap();
        runtime.block_on(server(handle, addr)).unwrap();
    }
}

#[derive(Debug)]
pub enum Error {
    Spawn {
        source: tokio_executor::SpawnError,
    },
    RuntimeBuild {
        source: io::Error,
    },
    CurrentThreadRun {
        source: tokio_executor::current_thread::RunError,
    },
}

#[async_trait]
pub trait Environment: Unpin + Sized + Clone + Send + 'static {
    type TcpStream: TcpStream + Send + 'static + Unpin;
    type TcpListener: TcpListener + Send + 'static + Unpin;

    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static;
    /// Return the time now according to the executor.
    fn now(&self) -> time::Instant;
    /// Returns a delay future which completes after the provided instant.
    fn delay(&self, deadline: time::Instant) -> tokio_timer::Delay;
    /// Returns a delay future which completes at some time from now.
    fn delay_from(&self, from_now: time::Duration) -> tokio_timer::Delay {
        let now = self.now();
        self.delay(now + from_now)
    }
    /// Creates a timeout future which will execute blah blah
    fn timeout<T>(&self, value: T, timeout: time::Duration) -> tokio_timer::Timeout<T>;

    async fn bind<A>(&self, addr: A) -> io::Result<Self::TcpListener>
    where
        A: Into<net::SocketAddr> + Send + Sync;
    async fn connect<A>(&self, addr: A) -> io::Result<Self::TcpStream>
    where
        A: Into<net::SocketAddr> + Send + Sync;
}

pub trait TcpStream: AsyncRead + AsyncWrite + Unpin {
    fn local_addr(&self) -> io::Result<net::SocketAddr>;
    fn peer_addr(&self) -> io::Result<net::SocketAddr>;
    fn shutdown(&self) -> io::Result<()>;
}

#[async_trait]
pub trait TcpListener {
    type Stream: TcpStream + Send;
    async fn accept(&mut self) -> Result<(Self::Stream, net::SocketAddr), io::Error>;
    fn local_addr(&self) -> Result<net::SocketAddr, io::Error>;
    fn ttl(&self) -> io::Result<u32>;
    fn set_ttl(&self, ttl: u32) -> io::Result<()>;
}

pub fn spawn_with_result<F, E, U>(env: &E, future: F) -> impl Future<Output = U>
where
    F: Future<Output = U> + Send + 'static,
    U: Send + 'static,
    E: Environment,
{
    let (remote, handle) = future.remote_handle();
    env.spawn(remote);
    Box::new(handle)
}
