use async_nats::{Client as NatsClient, jetstream::Context as NatsJetStream};
use redis::Client as RedisClient;
use sqlx::PgPool;

use crate::{
    db_router::{DbRouter, ReadConsistency},
    state::AppState,
};

pub trait HasDbRouter {
    fn db_router(&self) -> &DbRouter;

    fn writer_pool(&self) -> &PgPool {
        self.db_router().writer()
    }

    fn reader_pool(&self, consistency: ReadConsistency) -> &PgPool {
        self.db_router().reader(consistency)
    }
}

pub trait HasRedis {
    fn redis_client(&self) -> &RedisClient;
}

pub trait HasNatsClient {
    fn nats_client(&self) -> &NatsClient;
}

pub trait HasNatsJetStream {
    fn nats_jetstream(&self) -> &NatsJetStream;
}

pub trait HasIdGenerator {
    fn id_generator(&self) -> &sonyflake::Sonyflake;
}

impl HasDbRouter for AppState {
    fn db_router(&self) -> &DbRouter {
        &self.db_router
    }
}

impl HasRedis for AppState {
    fn redis_client(&self) -> &RedisClient {
        &self.redis_client
    }
}

impl HasNatsClient for AppState {
    fn nats_client(&self) -> &NatsClient {
        &self.nats_client
    }
}

impl HasNatsJetStream for AppState {
    fn nats_jetstream(&self) -> &NatsJetStream {
        &self.nats_jetstream
    }
}

impl HasIdGenerator for AppState {
    fn id_generator(&self) -> &sonyflake::Sonyflake {
        &self.sf
    }
}
