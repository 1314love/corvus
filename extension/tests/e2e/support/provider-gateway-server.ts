import { createHash, randomBytes } from 'node:crypto';
import { createServer, type IncomingMessage, type ServerResponse, type Server } from 'node:http';
import type { Duplex } from 'node:stream';
import { TestGateway } from '../../unit/provider-gateway';

const WEBSOCKET_GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';
const MAX_FRAME_BYTES = 24 * 1024 * 1024 + 1024;

export interface ProviderGatewayServer {
  gateway: TestGateway | null;
  token: Uint8Array;
  port: number;
  close: () => Promise<void>;
}

interface ParsedFrame {
  opcode: number;
  payload: Buffer;
  consumed: number;
}

function sendFrame(socket: Duplex, opcode: number, payload: Buffer): void {
  if (socket.destroyed) return;
  let header: Buffer;
  if (payload.length < 126) {
    header = Buffer.from([0x80 | opcode, payload.length]);
  } else if (payload.length <= 0xffff) {
    header = Buffer.alloc(4);
    header[0] = 0x80 | opcode;
    header[1] = 126;
    header.writeUInt16BE(payload.length, 2);
  } else {
    header = Buffer.alloc(10);
    header[0] = 0x80 | opcode;
    header[1] = 127;
    header.writeBigUInt64BE(BigInt(payload.length), 2);
  }
  socket.write(Buffer.concat([header, payload]));
}

function sendText(socket: Duplex, text: string): void {
  sendFrame(socket, 0x1, Buffer.from(text, 'utf8'));
}

function sendClose(socket: Duplex, code: number, reason: string): void {
  const payload = Buffer.alloc(2 + Buffer.byteLength(reason));
  payload.writeUInt16BE(code, 0);
  payload.write(reason, 2, 'utf8');
  sendFrame(socket, 0x8, payload);
  socket.end();
}

function parseFrame(buffer: Buffer): ParsedFrame | null {
  if (buffer.length < 2) return null;
  const first = buffer[0] ?? 0;
  const second = buffer[1] ?? 0;
  if ((first & 0x80) === 0) throw new Error('fragmented frames are not supported');
  const opcode = first & 0x0f;
  const masked = (second & 0x80) !== 0;
  let length = second & 0x7f;
  let offset = 2;
  if (length === 126) {
    if (buffer.length < 4) return null;
    length = buffer.readUInt16BE(2);
    offset = 4;
  } else if (length === 127) {
    if (buffer.length < 10) return null;
    const wide = buffer.readBigUInt64BE(2);
    if (wide > BigInt(MAX_FRAME_BYTES)) throw new Error('frame exceeds limit');
    length = Number(wide);
    offset = 10;
  }
  if (length > MAX_FRAME_BYTES) throw new Error('frame exceeds limit');
  if (masked) {
    if (buffer.length < offset + 4) return null;
    offset += 4;
  }
  if (buffer.length < offset + length) return null;
  const payload = Buffer.from(buffer.subarray(offset, offset + length));
  if (masked) {
    const maskOffset = offset - 4;
    for (let index = 0; index < payload.length; index += 1) {
      payload[index] = (payload[index] ?? 0) ^ (buffer[maskOffset + (index % 4)] ?? 0);
    }
  }
  return { opcode, payload, consumed: offset + length };
}

export async function startProviderGatewayServer(port: number): Promise<ProviderGatewayServer> {
  const token = new Uint8Array(randomBytes(32));
  let activeGateway: TestGateway | null = null;
  const sockets = new Set<Duplex>();

  const http: Server = createServer((_request: IncomingMessage, response: ServerResponse) => {
    response.statusCode = 404;
    response.end('not found');
  });

  http.on('upgrade', (request, socket) => {
    void (async () => {
      try {
        if (request.url !== '/provider/v2') {
          socket.write('HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n');
          socket.destroy();
          return;
        }
        const key = request.headers['sec-websocket-key'];
        if (typeof key !== 'string' || !key) {
          socket.destroy();
          return;
        }
        const accept = createHash('sha1')
          .update(key + WEBSOCKET_GUID)
          .digest('base64');
        socket.write(
          'HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n' +
            'Sec-WebSocket-Accept: ' +
            accept +
            '\r\n\r\n',
        );
        sockets.add(socket);
        const gateway = await TestGateway.create({ token });
        activeGateway = gateway;
        let closed = false;

        const sink = {
          emit(type: string, event: any): void {
            if (type === 'message') sendText(socket, String(event?.data ?? ''));
            else if (type === 'close') {
              if (!closed) {
                closed = true;
                sendClose(socket, Number(event?.code) || 1000, String(event?.reason ?? ''));
              }
            }
          },
        };
        (gateway.pair.server as any).peer = sink;

        let buffer = Buffer.alloc(0);
        socket.on('data', (chunk: Buffer) => {
          buffer = Buffer.concat([buffer, chunk]);
          try {
            for (;;) {
              const frame = parseFrame(buffer);
              if (!frame) break;
              buffer = buffer.subarray(frame.consumed);
              if (frame.opcode === 0x1) {
                gateway.pair.server.emit('message', { data: frame.payload.toString('utf8') });
              } else if (frame.opcode === 0x8) {
                gateway.pair.server.emit('close', { code: 1000, reason: 'client closed' });
                gateway.dispose();
                socket.end();
                break;
              } else if (frame.opcode === 0x9) {
                sendFrame(socket, 0xa, frame.payload);
              }
            }
          } catch {
            gateway.dispose();
            sendClose(socket, 1002, 'protocol error');
          }
        });
        socket.on('error', () => undefined);
        socket.on('close', () => {
          sockets.delete(socket);
          gateway.dispose();
        });

        gateway.pair.server.open();
      } catch {
        socket.destroy();
      }
    })();
  });

  await new Promise<void>((resolve, reject) => {
    http.once('error', reject);
    http.listen(port, '127.0.0.1', () => {
      http.off('error', reject);
      resolve();
    });
  });

  return {
    get gateway() {
      return activeGateway;
    },
    token,
    port,
    close: async () => {
      activeGateway?.dispose();
      for (const socket of sockets) socket.destroy();
      sockets.clear();
      http.closeAllConnections();
      await new Promise<void>((resolve) => http.close(() => resolve()));
    },
  };
}
