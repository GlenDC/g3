/*
 * Copyright 2023 ByteDance and/or its affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use cadence::{Counted, StatsdClient};

use crate::BackendStats;

pub(crate) fn emit_stats(client: &StatsdClient, s: &BackendStats) {
    macro_rules! emit_count {
        ($take:ident, $name:literal) => {
            let v = i64::try_from(s.$take()).unwrap_or(i64::MAX);
            client.count_with_tags(concat!("backend.", $name), v).send();
        };
    }

    emit_count!(take_refresh_total, "refresh_total");
    emit_count!(take_refresh_ok, "refresh_ok");
    emit_count!(take_request_total, "request_total");
    emit_count!(take_request_ok, "request_ok");
}
