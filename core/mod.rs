// Copyright 2024 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Core modules for the Verifiable Risk-Bound Basis Capture Agent (VRBCA).
//!
//! This module contains shared logic used by both the guest program and host application:
//! - Strategy: Basis capture and funding rate logic
//! - Risk: Delta-neutral and leverage constraints
//! - Mandate: Immutable trading mandate definitions
//! - State: Position tracking and PnL calculations

pub mod strategy;
pub mod risk;
pub mod mandate;
pub mod state;

// Re-export commonly used types
pub use strategy::*;
pub use risk::*;
pub use mandate::*;
pub use state::*;