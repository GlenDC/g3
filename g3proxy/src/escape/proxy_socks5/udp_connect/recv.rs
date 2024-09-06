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

use std::io;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use tokio::io::{AsyncRead, ReadBuf};
use tokio::net::TcpStream;

use g3_io_ext::{AsyncUdpRecv, LimitedStream, UdpCopyRemoteError, UdpCopyRemoteRecv};
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
))]
use g3_io_ext::{RecvMsgHdr, UdpCopyPacket, UdpCopyPacketMeta};
use g3_socks::v5::UdpInput;

pub(crate) struct ProxySocks5UdpConnectRemoteRecv<T> {
    inner: T,
    ctl_stream: LimitedStream<TcpStream>,
    end_on_control_closed: bool,
    ignore_ctl_stream: bool,
}

impl<T> ProxySocks5UdpConnectRemoteRecv<T>
where
    T: AsyncUdpRecv,
{
    pub(crate) fn new(
        recv: T,
        ctl_stream: LimitedStream<TcpStream>,
        end_on_control_closed: bool,
    ) -> Self {
        ProxySocks5UdpConnectRemoteRecv {
            inner: recv,
            ctl_stream,
            end_on_control_closed,
            ignore_ctl_stream: false,
        }
    }

    fn check_ctl_stream(&mut self, cx: &mut Context<'_>) -> Result<(), UdpCopyRemoteError> {
        const MAX_MSG_SIZE: usize = 4;
        let mut buf = [0u8; MAX_MSG_SIZE];

        let mut read_buf = ReadBuf::new(&mut buf);
        match Pin::new(&mut self.ctl_stream).poll_read(cx, &mut read_buf) {
            Poll::Pending => Ok(()),
            Poll::Ready(Ok(_)) => match read_buf.filled().len() {
                0 => {
                    if self.end_on_control_closed {
                        Err(UdpCopyRemoteError::RemoteSessionClosed)
                    } else {
                        self.ignore_ctl_stream = true;
                        Ok(())
                    }
                }
                MAX_MSG_SIZE => Err(UdpCopyRemoteError::RemoteSessionError(io::Error::other(
                    "unexpected data received in ctl stream",
                ))),
                _ => Ok(()), // drain extra data sent by some bad implementation
            },
            Poll::Ready(Err(e)) => Err(UdpCopyRemoteError::RemoteSessionError(e)),
        }
    }
}

impl<T> UdpCopyRemoteRecv for ProxySocks5UdpConnectRemoteRecv<T>
where
    T: AsyncUdpRecv,
{
    fn max_hdr_len(&self) -> usize {
        256 + 4 + 2
    }

    fn poll_recv_packet(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<(usize, usize), UdpCopyRemoteError>> {
        if !self.ignore_ctl_stream {
            self.check_ctl_stream(cx)?;
        }

        let nr = ready!(self.inner.poll_recv(cx, buf)).map_err(UdpCopyRemoteError::RecvFailed)?;

        let (off, _upstream) = UdpInput::parse_header(buf)
            .map_err(|e| UdpCopyRemoteError::InvalidPacket(e.to_string()))?;

        self.end_on_control_closed = true;
        Poll::Ready(Ok((off, nr)))
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
    ))]
    fn poll_recv_packets(
        &mut self,
        cx: &mut Context<'_>,
        packets: &mut [UdpCopyPacket],
    ) -> Poll<Result<usize, UdpCopyRemoteError>> {
        if !self.ignore_ctl_stream {
            self.check_ctl_stream(cx)?;
        }

        let mut hdr_v: Vec<RecvMsgHdr<1>> = packets
            .iter_mut()
            .map(|p| RecvMsgHdr::new([io::IoSliceMut::new(p.buf_mut())]))
            .collect();

        let count = ready!(self.inner.poll_batch_recvmsg(cx, &mut hdr_v))
            .map_err(UdpCopyRemoteError::RecvFailed)?;

        let mut r = Vec::with_capacity(count);
        for h in hdr_v.into_iter().take(count) {
            let iov = &h.iov[0];
            let (off, _upstream) = UdpInput::parse_header(&iov[0..h.n_recv])
                .map_err(|e| UdpCopyRemoteError::InvalidPacket(e.to_string()))?;
            r.push(UdpCopyPacketMeta::new(iov, off, h.n_recv));
        }
        for (m, p) in r.into_iter().zip(packets.iter_mut()) {
            m.set_packet(p);
        }

        self.end_on_control_closed = true;
        Poll::Ready(Ok(count))
    }
}
