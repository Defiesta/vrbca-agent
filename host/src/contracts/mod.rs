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

//! Smart contract interfaces for market data collection.
//!
//! This module provides contract bindings for interacting with:
//! - Uniswap V3 pools for on-chain price discovery
//! - Chainlink oracles for price feeds
//! - Other DeFi protocols for market data

pub mod uniswap_v3;

pub use uniswap_v3::*;