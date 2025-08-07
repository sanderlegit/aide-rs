use crate::error::Result;
use async_trait::async_trait;

pub mod impl_agent;
pub mod plan_agent;
pub mod state;

#[async_trait]
pub trait Agent {
    type Input;
    type Output;

    async fn run(&self, input: Self::Input) -> Result<Self::Output>;
}
