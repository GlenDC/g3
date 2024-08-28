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

use anyhow::anyhow;
use slog::slog_info;
use tokio::io::AsyncWriteExt;

use g3_dpi::ProtocolInspectPolicy;
use g3_io_ext::LimitedWriteExt;
use g3_slog_types::{LtUpstreamAddr, LtUuid};
use g3_types::net::{UpstreamAddr, WebSocketContext};

use super::{ClientCloseFrame, ServerCloseFrame};
#[cfg(feature = "quic")]
use crate::audit::StreamDetourContext;
use crate::config::server::ServerConfig;
use crate::inspect::{BoxAsyncRead, BoxAsyncWrite, StreamInspectContext};
use crate::serve::{ServerTaskError, ServerTaskResult};

macro_rules! intercept_log {
    ($obj:tt, $($args:tt)+) => {
        slog_info!($obj.ctx.intercept_logger(), $($args)+;
            "intercept_type" => "H1Websocket",
            "task_id" => LtUuid($obj.ctx.server_task_id()),
            "depth" => $obj.ctx.inspection_depth,
            "upstream" => LtUpstreamAddr(&$obj.upstream),
        )
    };
}

struct H1WebsocketIo {
    pub(crate) clt_r: BoxAsyncRead,
    pub(crate) clt_w: BoxAsyncWrite,
    pub(crate) ups_r: BoxAsyncRead,
    pub(crate) ups_w: BoxAsyncWrite,
}

pub(crate) struct H1WebsocketInterceptObject<SC: ServerConfig> {
    io: Option<H1WebsocketIo>,
    pub(crate) ctx: StreamInspectContext<SC>,
    upstream: UpstreamAddr,
    websocket_ctx: WebSocketContext,
}

impl<SC: ServerConfig> H1WebsocketInterceptObject<SC> {
    pub(crate) fn new(
        ctx: StreamInspectContext<SC>,
        upstream: UpstreamAddr,
        websocket_ctx: WebSocketContext,
    ) -> Self {
        H1WebsocketInterceptObject {
            io: None,
            ctx,
            upstream,
            websocket_ctx,
        }
    }

    pub(crate) fn set_io(
        &mut self,
        clt_r: BoxAsyncRead,
        clt_w: BoxAsyncWrite,
        ups_r: BoxAsyncRead,
        ups_w: BoxAsyncWrite,
    ) {
        let io = H1WebsocketIo {
            clt_r,
            clt_w,
            ups_r,
            ups_w,
        };
        self.io = Some(io);
    }

    pub(crate) async fn intercept(mut self) -> ServerTaskResult<()> {
        let r = match self.ctx.websocket_inspect_policy() {
            ProtocolInspectPolicy::Intercept => self.do_intercept().await,
            #[cfg(feature = "quic")]
            ProtocolInspectPolicy::Detour => self.do_detour().await,
            ProtocolInspectPolicy::Bypass => self.do_bypass().await,
            ProtocolInspectPolicy::Block => self.do_block().await,
        };
        match r {
            Ok(_) => {
                intercept_log!(self, "finished");
                Ok(())
            }
            Err(e) => {
                intercept_log!(self, "{e}");
                Err(e)
            }
        }
    }

    #[cfg(feature = "quic")]
    async fn do_detour(&mut self) -> ServerTaskResult<()> {
        let Some(client) = self.ctx.audit_handle.stream_detour_client() else {
            return self.do_bypass().await;
        };

        let H1WebsocketIo {
            clt_r,
            clt_w,
            ups_r,
            ups_w,
        } = self.io.take().unwrap();

        let mut ctx = StreamDetourContext::new(
            &self.ctx.server_config,
            &self.ctx.server_quit_policy,
            &self.ctx.task_notes,
            &self.upstream,
            g3_dpi::Protocol::Websocket,
        );
        ctx.set_payload(self.websocket_ctx.serialize());

        client.detour_relay(clt_r, clt_w, ups_r, ups_w, ctx).await
    }

    async fn do_bypass(&mut self) -> ServerTaskResult<()> {
        let H1WebsocketIo {
            clt_r,
            clt_w,
            ups_r,
            ups_w,
        } = self.io.take().unwrap();

        crate::inspect::stream::transit_transparent(
            clt_r,
            clt_w,
            ups_r,
            ups_w,
            &self.ctx.server_config,
            &self.ctx.server_quit_policy,
            self.ctx.user(),
        )
        .await
    }

    async fn do_block(&mut self) -> ServerTaskResult<()> {
        const SERVER_CLOSE_BYTES: [u8; 4] = ServerCloseFrame::encode_with_status_code(1001);
        const CLIENT_CLOSE_BYTES: [u8; 8] = ClientCloseFrame::encode_with_status_code(1001);

        let H1WebsocketIo {
            clt_r: _,
            mut clt_w,
            ups_r: _,
            mut ups_w,
        } = self.io.take().unwrap();

        tokio::spawn(async move {
            if ups_w.write_all_flush(&CLIENT_CLOSE_BYTES).await.is_ok() {
                let _ = ups_w.shutdown().await;
            }
        });

        if clt_w.write_all_flush(&SERVER_CLOSE_BYTES).await.is_ok() {
            let _ = clt_w.shutdown().await;
        }
        Err(ServerTaskError::InternalAdapterError(anyhow!(
            "websocket blocked by inspection policy"
        )))
    }

    async fn do_intercept(&mut self) -> ServerTaskResult<()> {
        let H1WebsocketIo {
            clt_r,
            clt_w,
            ups_r,
            ups_w,
        } = self.io.take().unwrap();

        crate::inspect::stream::transit_transparent(
            clt_r,
            clt_w,
            ups_r,
            ups_w,
            &self.ctx.server_config,
            &self.ctx.server_quit_policy,
            self.ctx.user(),
        )
        .await
    }
}
