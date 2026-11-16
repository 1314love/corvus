# corvus-cdp-worker manual smoke path

The worker is a stdio child process. It has no listening port and no authentication;
the gateway owns the browser endpoint and passes it in the worker_hello frame.

1. Start an isolated browser (never the user's normal Edge on 9222):

   ~~~bash
   CHROME=C:/Users/You/AppData/Local/Temp/corvus-chrome/chrome-win64/chrome.exe
   "$CHROME" --headless=new --disable-gpu --remote-debugging-port=9476 \
     --user-data-dir=C:/Users/You/AppData/Local/Temp/corvus-smoke-profile about:blank &
   ~~~

2. Read the browser WebSocket endpoint from the isolated port:

   ~~~bash
   sleep 2
   ENDPOINT=$(node -e "fetch('http://127.0.0.1:9476/json/version')\
     .then(r => r.json()).then(v => console.log(v.webSocketDebuggerUrl))")
   ~~~

3. Build both binaries and run the example against the worker:

   ~~~bash
   cargo build -p corvus-cdp-worker
   CORVUS_CDP_ENDPOINT="$ENDPOINT" \
     cargo run -p corvus-cdp-worker --example worker_smoke -- \
     target/debug/corvus-cdp-worker.exe
   ~~~

The example sends worker_hello, prints worker_ack and the cdp.get_version result,
then sends shutdown and checks exit status. The unit tests also cover a fake CDP
endpoint and a failed handshake without a browser.
