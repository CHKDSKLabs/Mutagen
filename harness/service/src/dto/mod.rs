//! Wire-side DTOs (INV-E3 / INV-E4 / MD-7 / ADR-0002).
//!
//! Every type that crosses the HTTP boundary lives here and derives
//! `utoipa::ToSchema` so the OpenAPI generator can see it. Domain types from
//! `mutagen-core` are translated into these shapes at the route boundary —
//! they don't leak through.

pub mod error;
pub mod health;
pub mod project;
pub mod session;
pub mod workflow;
pub mod workflow_commands;

pub use error::ErrorEnvelope;
pub use health::{HealthDto, VersionDto};
pub use project::{ProjectDto, ProjectListDto, RegisterProjectRequest};
pub use session::{ClientMessage, QuestionEnvelopeDto, ServerMessage};
pub use workflow::{
    OriginDto, SliceCountsDto, SliceDto, SliceQueueDto, StateLogPageDto, StateUpdateDto, StatusDto,
};
pub use workflow_commands::{
    CommandAcceptedDto, ConfirmEscalateDto, EmptyCommandBody, EscalateRequest,
};
