import { createHash } from 'node:crypto';
import { createServer, type Server } from 'node:http';
import type { Duplex } from 'node:stream';

const WS_GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';

interface Frame {
  opcode: number;
  payload: Buffer;
  rest: Buffer;
}

interface HelloWaiter {
  resolve: (frame: Record<string, unknown>) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout> | null;
}

interface ResponseWaiter {
  resolve: (frame: Record<string, unknown>) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout> | null;
}

export interface MockDaemonSnapshot {
  connections: number;
  hellos: number;
  lastUpgradeAt: string | null;
}

export interface MockDaemon {
  port: number;
  hellos: Array<Record<string, unknown>>;
  snapshot: () => MockDaemonSnapshot;
  waitForHello: (timeoutMs?: number) => Promise<Record<string, unknown>>;
  sendCommand: (tool: string, args: Record<string, unknown>, timeoutMs?: number) => Promise<Record<string, unknown>>;
  close: () => Promise<void>;
}

function parseFrame(buffer: Buffer): Frame | null {
  if (buffer.length < 2) return null;
  const first = buffer[0];
  const second = buffer[1];
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
    length = Number(buffer.readBigUInt64BE(2));
    offset = 10;
  }

  const maskOffset = offset;
  if (masked) offset += 4;
  if (buffer.length < offset + length) return null;

  let payload = buffer.subarray(offset, offset + length);
  if (masked) {
    const mask = buffer.subarray(maskOffset, maskOffset + 4);
    payload = Buffer.from(payload.map((byte, index) => byte ^ mask[index % 4]));
  }

  return { opcode, payload, rest: buffer.subarray(offset + length) };
}

function encodeFrame(opcode: number, payload: Buffer): Buffer {
  if (payload.length < 126) return Buffer.concat([Buffer.from([0x80 | opcode, payload.length]), payload]);
  if (payload.length < 65536) {
    const header = Buffer.alloc(4);
    header[0] = 0x80 | opcode;
    header[1] = 126;
    header.writeUInt16BE(payload.length, 2);
    return Buffer.concat([header, payload]);
  }
  const header = Buffer.alloc(10);
  header[0] = 0x80 | opcode;
  header[1] = 127;
  header.writeBigUInt64BE(BigInt(payload.length), 2);
  return Buffer.concat([header, payload]);
}

function sendText(socket: Duplex, text: string): void {
  if (!socket.destroyed && socket.writable) socket.write(encodeFrame(0x1, Buffer.from(text)));
}

export async function startMockDaemon(port: number): Promise<MockDaemon> {
  const hellos: Array<Record<string, unknown>> = [];
  const sockets = new Set<Duplex>();
  const waiters = new Set<HelloWaiter>();
  const responseWaiters = new Map<string, ResponseWaiter>();
  let commandSeq = 0;
  let connections = 0;
  let lastUpgradeAt: number | null = null;

  const settleHello = (frame: Record<string, unknown>): void => {
    for (const waiter of [...waiters]) {
      waiters.delete(waiter);
      if (waiter.timer) clearTimeout(waiter.timer);
      waiter.resolve(frame);
    }
  };

  const rejectWaiters = (error: Error): void => {
    for (const waiter of [...waiters]) {
      waiters.delete(waiter);
      if (waiter.timer) clearTimeout(waiter.timer);
      waiter.reject(error);
    }
  };

  const settleResponse = (frame: Record<string, unknown>): void => {
    const id = typeof frame.id === 'string' ? frame.id : null;
    if (!id) return;
    const waiter = responseWaiters.get(id);
    if (!waiter) return;
    responseWaiters.delete(id);
    if (waiter.timer) clearTimeout(waiter.timer);
    waiter.resolve(frame);
  };

  const server: Server = createServer((_request, response) => {
    response.statusCode = 404;
    response.end('not a daemon route');
  });

  server.on('upgrade', (request, socket) => {
    const key = request.headers['sec-websocket-key'];
    if (typeof key !== 'string') {
      socket.destroy();
      return;
    }

    const accept = createHash('sha1')
      .update(key + WS_GUID)
      .digest('base64');
    socket.write(
      'HTTP/1.1 101 Switching Protocols\r\n' +
        'Upgrade: websocket\r\n' +
        'Connection: Upgrade\r\n' +
        'Sec-WebSocket-Accept: ' +
        accept +
        '\r\n\r\n',
    );
    connections += 1;
    lastUpgradeAt = Date.now();
    sockets.add(socket);
    socket.on('error', () => undefined);
    socket.on('close', () => sockets.delete(socket));

    let pending: Buffer<ArrayBufferLike> = Buffer.alloc(0);
    socket.on('data', (chunk) => {
      pending = Buffer.concat([pending, chunk]);
      for (;;) {
        const frame = parseFrame(pending);
        if (!frame) break;
        pending = frame.rest;

        if (frame.opcode === 0x8) {
          socket.end(encodeFrame(0x8, Buffer.alloc(0)));
          break;
        }
        if (frame.opcode === 0x9) {
          socket.write(encodeFrame(0xa, frame.payload));
          continue;
        }
        if (frame.opcode !== 0x1) continue;

        let parsed: Record<string, unknown>;
        try {
          parsed = JSON.parse(frame.payload.toString('utf8')) as Record<string, unknown>;
        } catch {
          continue;
        }

        if (parsed.kind === 'ping') {
          sendText(socket, JSON.stringify({ v: 1, id: parsed.id, kind: 'pong' }));
          continue;
        }
        if (parsed.kind === 'response') {
          settleResponse(parsed);
          continue;
        }
        if (parsed.kind !== 'hello') continue;

        hellos.push(parsed);
        sendText(
          socket,
          JSON.stringify({
            v: 1,
            id: parsed.id,
            kind: 'hello_ack',
            browser: 'chromium:e2e',
            args: {
              browser_id: 'chromium:e2e',
              daemon_version: 'e2e-1.0.0',
              limits: {},
            },
          }),
        );
        settleHello(parsed);
      }
    });
  });

  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(port, '127.0.0.1', () => {
      server.off('error', reject);
      resolve();
    });
  });

  return {
    port,
    hellos,
    snapshot: () => ({
      connections,
      hellos: hellos.length,
      lastUpgradeAt: lastUpgradeAt === null ? null : new Date(lastUpgradeAt).toISOString(),
    }),
    waitForHello: (timeoutMs = 20000) => {
      if (hellos.length > 0) return Promise.resolve(hellos[0]);
      return new Promise<Record<string, unknown>>((resolve, reject) => {
        const waiter: HelloWaiter = { resolve, reject, timer: null };
        const timer = setTimeout(() => {
          waiters.delete(waiter);
          reject(
            new Error(
              'timed out waiting for a daemon hello after ' +
                String(timeoutMs) +
                ' ms (connections=' +
                String(connections) +
                ')',
            ),
          );
        }, timeoutMs);
        waiter.timer = timer;
        timer.unref?.();
        waiters.add(waiter);
      });
    },
    sendCommand: (tool, args, timeoutMs = 20000) => {
      const socket = [...sockets].find((candidate) => !candidate.destroyed && candidate.writable);
      if (!socket) return Promise.reject(new Error('mock daemon has no connected socket'));
      const id = 'cmd-' + String((commandSeq += 1));
      return new Promise<Record<string, unknown>>((resolve, reject) => {
        const waiter: ResponseWaiter = { resolve, reject, timer: null };
        responseWaiters.set(id, waiter);
        const timer = setTimeout(() => {
          if (!responseWaiters.has(id)) return;
          responseWaiters.delete(id);
          reject(new Error('timed out waiting for a response to ' + tool + ' after ' + String(timeoutMs) + ' ms'));
        }, timeoutMs);
        waiter.timer = timer;
        timer.unref?.();
        sendText(
          socket,
          JSON.stringify({
            v: 1,
            id,
            kind: 'command',
            tool,
            args,
            meta: { timeout_ms: timeoutMs },
          }),
        );
      });
    },
    close: async () => {
      rejectWaiters(new Error('mock daemon closed'));
      for (const [id, waiter] of responseWaiters) {
        responseWaiters.delete(id);
        if (waiter.timer) clearTimeout(waiter.timer);
        waiter.reject(new Error('mock daemon closed'));
      }
      for (const socket of sockets) socket.destroy();
      await new Promise<void>((resolve) => server.close(() => resolve()));
    },
  };
}
