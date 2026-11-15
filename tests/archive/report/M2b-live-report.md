# Corvus M2b live acceptance report

Status: **PASS** — `passed=105 failed=0` (83 happy-path checkpoints and 22 negative checkpoints; 2 additional scenarios skipped; 0 failed). All 26 frozen tools were called at least once against the live Corvus Windows service and the real Edge browser session. No product defects were found.

Run window: **2026-11-14 17:51:23–17:51:26 UTC+8** (3.7 s). QA did not start, stop or restart the Corvus service; QA started only its own fixture server on port 8812.

## 1. Environment

| field | observed value |
|---|---|
| Run id | `2026-11-14-qa-live-m2b-1` |
| Date and time | 2026-11-14 17:51:23–17:51:26 UTC+8 |
| Operator | qa |
| Commit | `7ead4c8` |
| Host OS | Microsoft Windows 10.0.19045.6466 |
| Browser | Microsoft Edge 157.0.4341.46 (branded Edge; extension loaded unpacked) |
| Extension id | `pdgemoghkehbjpklbmkmchgfmpfghbl` |
| Extension version | `0.1.0` |
| Daemon | `corvusd 0.1.0`, protocol `2025-06-18`, MCP `http://127.0.0.1:7331/mcp`, WS `127.0.0.1:7330` (existing Windows service) |
| Browser id / profile | `edge:33628f01180d3d48` / `33628f01180d3d48` |
| Capabilities | `["dom","scripting","debugger"]` |
| `connected_at_ms` | `1794649303326` |
| Fixture server | `http://127.0.0.1:8812` (QA-owned `tests/acceptance/fixtures/serve.mjs`) |
| Fixture routes used | `/m2b`, `/m2`, `/m2-b`, `/api/json`, `/api/slow`, `/download/report.txt` |
| Frozen tool count | 26; `tools/list` returned exactly 26 names and optional string `browser_id` on every schema |
| Test tab / previous tab | own tab `1773823980`; previous active tab `1773823880` restored |
| Evidence | `C:\Users\ADMINI~1\AppData\Local\Temp\corvus-m2b-live\evidence` (105 JSON files), plus `summary.json` and `report-data.json` |

## 2. Result summary

| item | result |
|---|---|
| Happy-path checkpoints | 83 / 83 PASS |
| Negative/error-path checkpoints | 22 / 22 PASS |
| Total checkpoints | 105 / 105 PASS |
| Failed | 0 |
| Skipped | 2 (listed in section 6) |
| Product defects | 0 |
| Tools exercised | 26 / 26 |

Every page or action checkpoint passed an explicit own tab id where the tool accepts `tab_id`; tab-list and download-list checkpoints do not take one. One checkpoint is one MCP `tools/call`; asynchronous page effects were verified by a following `browser_wait`/`page_text`/`browser_eval` checkpoint, not by sleeps. The page was reached through the existing daemon and the real unpacked extension.

## 3. Happy-path checkpoints

`exact arguments` is the JSON object passed to `tools/call`. `observed result` is the parsed text payload trimmed to fields that prove the checkpoint. Latency is measured around the HTTP call in the driver. Pre-existing tab URLs/titles and pre-existing download filenames are shown as placeholders in this report; the raw checkpoint JSON retains the observed values.

| # | tool | exact arguments | observed result (proof fields) | latency (ms) | evidence | result |
|---|---|---|---|---|---|---|
| 1 | `browser_list` | `{}` | `{"browsers":1,"browser_id":"edge:33628f01180d3d48","capabilities":["dom","scripting","debugger"]}` | 3 | `001-browser_list_one_browser.json` | PASS |
| 2 | `tab_list` | `{}` | `{"tabs":2,"active":"1773823880","urls":["<pre-existing tab URL>","<pre-existing tab URL>"]}` | 4 | `002-tab_list_before_creating_own_tab.json` | PASS |
| 3 | `tab_new` | `{"url":"http://127.0.0.1:8812/m2b","active":true}` | `{"tab_id":"1773823980","url":"http://127.0.0.1:8812/m2b","loaded":true,"previous_tab_id":"1773823880"}` | 214 | `003-tab_new_own_m2b_tab.json` | PASS |
| 4 | `browser_navigate` | `{"url":"http://127.0.0.1:8812/m2b","tab_id":"1773823980"}` | `{"navigated":true,"loaded":true,"url":"http://127.0.0.1:8812/m2b"}` | 183 | `004-browser_navigate_m2b_own_tab.json` | PASS |
| 5 | `page_snapshot` | `{"tab_id":"1773823980","mode":"interactive"}` | `{"title":"Corvus M2b Page","url":"http://127.0.0.1:8812/m2b","nodes":8,"has_fetch_json":true}` | 8 | `005-page_snapshot_m2b.json` | PASS |
| 6 | `page_text` | `{"tab_id":"1773823980"}` | `{"title":"Corvus M2b Page","marker":"M2b fixture marker 5b21","text_chars":169}` | 13 | `006-page_text_m2b_marker.json` | PASS |
| 7 | `page_html` | `{"tab_id":"1773823980","max_chars":50000}` | `{"html_chars":2004,"has_file_input":true,"truncated":false}` | 8 | `007-page_html_m2b_hidden_file_input.json` | PASS |
| 8 | `page_find` | `{"query":"Fetch JSON","tab_id":"1773823980"}` | `{"count":1,"ref":"@e2","tag":"button","text":"Fetch JSON"}` | 4 | `008-page_find_Fetch_JSON_ref.json` | PASS |
| 9 | `page_text` | `{"browser_id":"edge:33628f01180d3d48","tab_id":"1773823980"}` | `{"browser_id":"edge:33628f01180d3d48","routed":true}` | 6 | `009-page_text_explicit_browser_id_routing.json` | PASS |
| 10 | `net_capture` | `{"action":"start","tab_id":"1773823980"}` | `{"capturing":true,"tab_id":"1773823980","capacity":2000,"started_at":1794649883531}` | 8 | `010-net_capture_start.json` | PASS |
| 11 | `browser_navigate` | `{"url":"http://127.0.0.1:8812/m2b?capture=1","tab_id":"1773823980"}` | `{"loaded":true,"url":"http://127.0.0.1:8812/m2b?capture=1"}` | 181 | `011-browser_navigate_with_capture_active.json` | PASS |
| 12 | `net_capture` | `{"action":"status","tab_id":"1773823980"}` | `{"capturing":true,"tab_id":"1773823980","entries":2,"started_at":1794649883531}` | 26 | `012-net_capture_status_after_navigation.json` | PASS |
| 13 | `page_find` | `{"query":"Fetch JSON","tab_id":"1773823980"}` | `{"ref":"@e1","tag":"button","text":"Fetch JSON"}` | 20 | `013-page_find_Fetch_JSON_after_capture_navigate.json` | PASS |
| 14 | `browser_click` | `{"target":"@e1","tab_id":"1773823980"}` | `{"clicked":true,"ref":"@e1","tag":"button","events":["pointerover","pointerenter","pointermove","pointerdown","mousedown","pointerup","mouseup","click"],"url":"http://127.0.0.1:8812/m2b?capture=1"}` | 18 | `014-browser_click_fetch-json.json` | PASS |
| 15 | `browser_wait` | `{"text":"json:7","timeout_ms":5000,"tab_id":"1773823980"}` | `{"matched":true,"waited_ms":5,"url":"http://127.0.0.1:8812/m2b?capture=1"}` | 23 | `015-browser_wait_json_7.json` | PASS |
| 16 | `page_find` | `{"query":"Fetch slow JSON","tab_id":"1773823980"}` | `{"ref":"@e2","tag":"button","text":"Fetch slow JSON"}` | 5 | `016-page_find_Fetch_slow_JSON.json` | PASS |
| 17 | `browser_click` | `{"target":"@e2","tab_id":"1773823980"}` | `{"clicked":true,"ref":"@e2","tag":"button","url":"http://127.0.0.1:8812/m2b?capture=1"}` | 11 | `017-browser_click_fetch-slow.json` | PASS |
| 18 | `browser_wait` | `{"text":"slow:42","timeout_ms":5000,"tab_id":"1773823980"}` | `{"matched":true,"waited_ms":675,"url":"http://127.0.0.1:8812/m2b?capture=1"}` | 678 | `018-browser_wait_slow_42.json` | PASS |
| 19 | `net_log` | `{"tab_id":"1773823980","url_contains":"/api/","limit":20}` | `{"entries":2,"next_seq":50,"json_status":200,"json_duration_ms":23,"slow_duration_ms":616,"headers_absent":true}` | 6 | `019-net_log_api_entries.json` | PASS |
| 20 | `net_log` | `{"tab_id":"1773823980","url_contains":"/api/json","include_headers":true,"limit":5}` | `{"entries":1,"request_headers":6,"response_headers":6,"next_seq":49}` | 3 | `020-net_log_headers_gated.json` | PASS |
| 21 | `net_log` | `{"tab_id":"1773823980","limit":1}` | `{"entries":1,"seq":47,"next_seq":47}` | 3 | `021-net_log_limit_1.json` | PASS |
| 22 | `net_log` | `{"tab_id":"1773823980","since_seq":49,"limit":1}` | `{"since_seq":49,"seq":50,"next_seq":50}` | 3 | `022-net_log_since_seq.json` | PASS |
| 23 | `net_log` | `{"tab_id":"1773823980","limit":1000}` | `{"entries":4,"next_seq":50}` | 5 | `023-net_log_full_count_before_clear.json` | PASS |
| 24 | `net_clear` | `{"tab_id":"1773823980"}` | `{"cleared":4}` | 3 | `024-net_clear_captured_tab.json` | PASS |
| 25 | `net_log` | `{"tab_id":"1773823980"}` | `{"entries":0,"next_seq":0,"capturing":true}` | 3 | `025-net_log_empty_after_clear.json` | PASS |
| 26 | `net_intercept` | `{"action":"set","tab_id":"1773823980","rules":[{"id":"block-json","action":"block","url_regex":"/api/json"}]}` | `{"ok":true,"scope":"tab","applied":true,"rules":["block-json"]}` | 23 | `034-net_intercept_set_block_rule.json` | PASS |
| 27 | `browser_click` | `{"target":"@e1","tab_id":"1773823980"}` | `{"clicked":true,"ref":"@e1"}` | 25 | `035-browser_click_fetch-json_with_block_rule.json` | PASS |
| 28 | `browser_wait` | `{"text":"network failed","timeout_ms":5000,"tab_id":"1773823980"}` | `{"matched":true,"waited_ms":2}` | 5 | `036-browser_wait_blocked_failure_text.json` | PASS |
| 29 | `browser_eval` | `{"expression":"document.querySelector(\"#net-out\").dataset.net","world":"isolated","tab_id":"1773823980"}` | `{"value":"failed"}` | 13 | `037-blocked_page_data-net_failed.json` | PASS |
| 30 | `page_text` | `{"tab_id":"1773823980"}` | `{"contains_network_failed":true,"text_chars":188}` | 6 | `038-page_text_blocked_failure.json` | PASS |
| 31 | `net_log` | `{"tab_id":"1773823980","url_contains":"/api/json","limit":10}` | `{"seq":51,"status":null,"error":"net::ERR_BLOCKED_BY_CLIENT.Inspector","intercepted":{"action":"block","rule_id":"block-json"}}` | 4 | `039-net_log_block_entry.json` | PASS |
| 32 | `net_intercept` | `{"action":"set","tab_id":"1773823980","rules":[{"id":"mock-json","action":"mock","url_regex":"/api/json","status":200,"headers":{"content-type":"application/json"},"body_base64":"eyJvayI6dHJ1ZSwic291cmNlIjoibW9jayIsInZhbHVlIjo5OX0="}]}` | `{"ok":true,"scope":"tab","applied":true,"rules":["mock-json"]}` | 4 | `040-net_intercept_set_mock_rule_replaces_block.json` | PASS |
| 33 | `browser_click` | `{"target":"@e1","tab_id":"1773823980"}` | `{"clicked":true,"ref":"@e1"}` | 6 | `041-browser_click_fetch-json_with_mock_rule.json` | PASS |
| 34 | `browser_wait` | `{"text":"mock:99","timeout_ms":5000,"tab_id":"1773823980"}` | `{"matched":true,"waited_ms":3}` | 10 | `042-browser_wait_mock_99.json` | PASS |
| 35 | `browser_eval` | `{"expression":"document.querySelector(\"#net-out\").dataset.net","world":"isolated","tab_id":"1773823980"}` | `{"value":"mock:99"}` | 4 | `043-mock_page_data-net_mock.json` | PASS |
| 36 | `page_text` | `{"tab_id":"1773823980"}` | `{"contains_mock":true,"text_chars":164}` | 6 | `044-page_text_mock_response.json` | PASS |
| 37 | `net_log` | `{"tab_id":"1773823980","url_contains":"/api/json","limit":10}` | `{"seq":52,"status":200,"mime":"application/json","intercepted":{"action":"mock","rule_id":"mock-json"}}` | 2 | `045-net_log_mock_entry.json` | PASS |
| 38 | `net_intercept` | `{"action":"clear","tab_id":"1773823980"}` | `{"ok":true,"scope":"tab","applied":true,"rules":[]}` | 4 | `046-net_intercept_clear_rules.json` | PASS |
| 39 | `browser_click` | `{"target":"@e1","tab_id":"1773823980"}` | `{"clicked":true,"ref":"@e1"}` | 8 | `047-browser_click_fetch-json_after_intercept_clear.json` | PASS |
| 40 | `browser_wait` | `{"text":"json:7","timeout_ms":5000,"tab_id":"1773823980"}` | `{"matched":true,"waited_ms":1}` | 7 | `048-browser_wait_json_7_after_clear.json` | PASS |
| 41 | `net_log` | `{"tab_id":"1773823980","url_contains":"/api/json","limit":10}` | `{"seq":53,"status":200,"intercepted":null,"duration_ms":6}` | 4 | `049-net_log_real_request_after_clear.json` | PASS |
| 42 | `net_capture` | `{"action":"stop","tab_id":"1773823980"}` | `{"capturing":false,"tab_id":"1773823980","entries":3}` | 3 | `050-net_capture_stop.json` | PASS |
| 43 | `download_list` | `{"limit":20}` | `{"downloads":7,"newest":{"id":7,"filename":"<pre-existing download>","state":"complete"}}` | 4 | `051-download_list_before_click.json` | PASS |
| 44 | `browser_click` | `{"target":"#download","tab_id":"1773823980"}` | `{"clicked":true,"tag":"a","url":"http://127.0.0.1:8812/m2b?capture=1"}` | 52 | `053-browser_click_download_link.json` | PASS |
| 45 | `download_wait` | `{"timeout_ms":20000}` | `{"id":8,"state":"complete","filename":"corvus-report.txt","bytes_received":24,"total_bytes":24}` | 217 | `054-download_wait_newest_after_click.json` | PASS |
| 46 | `download_list` | `{"limit":20}` | `{"id":8,"filename":"corvus-report.txt","state":"complete","bytes_received":24,"total_bytes":24,"exists":true}` | 3 | `055-download_list_after_click_contains_new_id.json` | PASS |
| 47 | `download_wait` | `{"id":"8","timeout_ms":5000}` | `{"id":8,"state":"complete","filename":"corvus-report.txt"}` | 9 | `056-download_wait_explicit_decimal_string_id.json` | PASS |
| 48 | `download_list` | `{"limit":1}` | `{"downloads":1,"id":8}` | 9 | `059-download_list_limit_1.json` | PASS |
| 49 | `upload_file` | `{"target":"#file","files":["C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt"],"tab_id":"1773823980"}` | `{"uploaded":true,"files":["C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt"],"target":{"ref":"@e4","tag":"input","text":""},"input":{"accept":".txt,text/plain","multiple":false}}` | 19 | `061-upload_file_hidden_file_input.json` | PASS |
| 50 | `browser_eval` | `{"expression":"Array.from(document.querySelector(\"#file\").files).map(f=>({name:f.name,size:f.size}))","world":"main","tab_id":"1773823980"}` | `{"name":"upload.txt","size":22}` | 5 | `062-browser_eval_input.files_main_world.json` | PASS |
| 51 | `page_text` | `{"tab_id":"1773823980"}` | `{"contains_files_upload":true}` | 8 | `063-page_text_upload_files_state.json` | PASS |
| 52 | `browser_navigate` | `{"url":"http://127.0.0.1:8812/m2","tab_id":"1773823980"}` | `{"navigated":true,"loaded":true,"url":"http://127.0.0.1:8812/m2"}` | 183 | `069-browser_navigate_M2_page.json` | PASS |
| 53 | `page_snapshot` | `{"tab_id":"1773823980","mode":"interactive"}` | `{"title":"Corvus M2 Page","url":"http://127.0.0.1:8812/m2","nodes":31}` | 31 | `070-page_snapshot_M2_page.json` | PASS |
| 54 | `page_text` | `{"tab_id":"1773823980"}` | `{"title":"Corvus M2 Page","text_chars":352}` | 19 | `071-page_text_M2_page.json` | PASS |
| 55 | `page_html` | `{"tab_id":"1773823980","max_chars":50000}` | `{"html_chars":2633}` | 19 | `072-page_html_M2_page_input_q.json` | PASS |
| 56 | `page_find` | `{"query":"Find target","tab_id":"1773823980"}` | `{"ref":"@e7","tag":"button","text":"Find target"}` | 4 | `073-page_find_Find_target_ref.json` | PASS |
| 57 | `browser_click` | `{"target":"@e7","tab_id":"1773823980"}` | `{"clicked":true,"ref":"@e7","tag":"button"}` | 10 | `074-browser_click_page_find_returned_ref.json` | PASS |
| 58 | `browser_hover` | `{"target":"#hover","tab_id":"1773823980"}` | `{"hovered":true,"target":{"ref":"@e3","tag":"button","text":"Hover me"},"url":"http://127.0.0.1:8812/m2"}` | 8 | `075-browser_hover_hover_button.json` | PASS |
| 59 | `page_text` | `{"tab_id":"1773823980"}` | `{"contains_hover_effect":true}` | 4 | `076-page_text_hover_effect.json` | PASS |
| 60 | `browser_press` | `{"target":"#keyinput","keys":["Enter","Tab"],"tab_id":"1773823980"}` | `{"pressed":true,"keys":["Enter","Tab"],"url":"http://127.0.0.1:8812/m2"}` | 6 | `077-browser_press_keys.json` | PASS |
| 61 | `page_text` | `{"tab_id":"1773823980"}` | `{"contains_press_log":true}` | 4 | `078-page_text_press_log.json` | PASS |
| 62 | `browser_scroll` | `{"direction":"down","pages":2,"tab_id":"1773823980"}` | `{"scrolled":true,"x":0,"y":1816}` | 7 | `079-browser_scroll_down.json` | PASS |
| 63 | `browser_scroll` | `{"direction":"up","pages":1,"tab_id":"1773823980"}` | `{"scrolled":true,"x":0,"y":908,"previous_y":1816}` | 8 | `080-browser_scroll_up.json` | PASS |
| 64 | `browser_select` | `{"target":"#single","values":["two"],"tab_id":"1773823980"}` | `{"selected":["two"],"value":"two","url":"http://127.0.0.1:8812/m2"}` | 5 | `081-browser_select_single_option.json` | PASS |
| 65 | `browser_type` | `{"target":"#q","text":"hello corvus","tab_id":"1773823980"}` | `{"typed":true,"value":"hello corvus","value_length":12,"events":["beforeinput","input","change"]}` | 22 | `082-browser_type_query_text.json` | PASS |
| 66 | `browser_click` | `{"target":"#go","tab_id":"1773823980"}` | `{"clicked":true,"tag":"button","events":["pointerover","pointerenter","pointermove","pointerdown","mousedown","pointerup","mouseup","click"],"url":"http://127.0.0.1:8812/m2"}` | 4 | `083-browser_click_run_button.json` | PASS |
| 67 | `page_text` | `{"tab_id":"1773823980"}` | `{"contains_clicked":true}` | 10 | `084-page_text_clicked_value.json` | PASS |
| 68 | `browser_eval` | `{"expression":"2+2","world":"isolated","tab_id":"1773823980"}` | `{"ok":true,"value":4}` | 6 | `085-browser_eval_isolated_2_2.json` | PASS |
| 69 | `browser_eval` | `{"expression":"document.title","world":"main","tab_id":"1773823980"}` | `{"ok":true,"value":"Corvus M2 Page"}` | 3 | `086-browser_eval_main_document.title.json` | PASS |
| 70 | `browser_wait` | `{"text":"clicked:hello corvus","timeout_ms":5000,"tab_id":"1773823980"}` | `{"matched":true,"waited_ms":1}` | 3 | `087-browser_wait_text_already_present.json` | PASS |
| 71 | `browser_navigate` | `{"url":"http://127.0.0.1:8812/m2-b","tab_id":"1773823980"}` | `{"navigated":true,"loaded":true,"url":"http://127.0.0.1:8812/m2-b"}` | 193 | `089-browser_history_navigate_to_page_B.json` | PASS |
| 72 | `browser_history` | `{"action":"back","tab_id":"1773823980"}` | `{"action":"back","url":"http://127.0.0.1:8812/m2","loaded":true}` | 64 | `090-browser_history_back.json` | PASS |
| 73 | `page_text` | `{"tab_id":"1773823980"}` | `{"url":"http://127.0.0.1:8812/m2","title":"Corvus M2 Page","contains_m2":true}` | 32 | `091-page_text_immediate_after_history_back.json` | PASS |
| 74 | `page_snapshot` | `{"tab_id":"1773823980","mode":"interactive"}` | `{"url":"http://127.0.0.1:8812/m2","title":"Corvus M2 Page","nodes":31}` | 16 | `092-page_snapshot_immediate_after_history_back.json` | PASS |
| 75 | `browser_history` | `{"action":"forward","tab_id":"1773823980"}` | `{"action":"forward","url":"http://127.0.0.1:8812/m2-b","loaded":true}` | 61 | `093-browser_history_forward.json` | PASS |
| 76 | `page_text` | `{"tab_id":"1773823980"}` | `{"url":"http://127.0.0.1:8812/m2-b","title":"Corvus M2 Page B","contains_b":true}` | 18 | `094-page_text_immediate_after_history_forward.json` | PASS |
| 77 | `page_snapshot` | `{"tab_id":"1773823980","mode":"interactive"}` | `{"url":"http://127.0.0.1:8812/m2-b","title":"Corvus M2 Page B","nodes":3}` | 14 | `095-page_snapshot_immediate_after_history_forward.json` | PASS |
| 78 | `browser_history` | `{"action":"reload","tab_id":"1773823980"}` | `{"action":"reload","url":"http://127.0.0.1:8812/m2-b","loaded":true}` | 104 | `096-browser_history_reload.json` | PASS |
| 79 | `page_text` | `{"tab_id":"1773823980"}` | `{"url":"http://127.0.0.1:8812/m2-b","title":"Corvus M2 Page B","contains_b":true}` | 25 | `097-page_text_immediate_after_reload.json` | PASS |
| 80 | `tab_list` | `{}` | `{"tabs":3,"own":"1773823980","previous":"1773823880","own_active":true}` | 6 | `098-tab_list_own_and_previous.json` | PASS |
| 81 | `tab_switch` | `{"tab_id":"1773823880"}` | `{"tab_id":"1773823880","url":"<pre-existing tab URL>","title":"<pre-existing tab title>","active":true}` | 46 | `103-tab_switch_restores_previous_tab.json` | PASS |
| 82 | `tab_close` | `{"tab_id":"1773823980"}` | `{"closed":true,"tab_id":"1773823980"}` | 52 | `104-tab_close_own_tab.json` | PASS |
| 83 | `tab_list` | `{}` | `{"tabs":2,"previous":"1773823880","previous_active":true}` | 5 | `105-tab_list_after_own_tab_closed.json` | PASS |

## 4. Negative and error taxonomy

All negative cases returned HTTP 200, `result.isError=true`, and text in the form `CODE: message`, with a trailing details JSON object where the tool supplied details. No JSON-RPC error was returned for a tool-level failure.

| # | tool | exact arguments | expected | observed code / message / details | latency (ms) | evidence | result |
|---|---|---|---|---|---|---|---|
| 1 | `net_capture` | `{"action":"pause","tab_id":"1773823980"}` | `BAD_ARGS` | `BAD_ARGS: net_capture action must be one of start, stop, status` | 1 | `026-net_capture_rejects_pause.json` | PASS |
| 2 | `net_log` | `{"limit":0,"tab_id":"1773823980"}` | `BAD_ARGS` | `BAD_ARGS: net_log argument limit must be an integer from 1 to 1000` | 2 | `027-net_log_rejects_limit_0.json` | PASS |
| 3 | `net_log` | `{"limit":1001,"tab_id":"1773823980"}` | `BAD_ARGS` | `BAD_ARGS: net_log argument limit must be an integer from 1 to 1000` | 1 | `028-net_log_rejects_limit_1001.json` | PASS |
| 4 | `net_log` | `{"status_min":500,"status_max":400,"tab_id":"1773823980"}` | `BAD_ARGS` | `BAD_ARGS: net_log status_min must not exceed status_max` | 2 | `029-net_log_rejects_inverted_status_range.json` | PASS |
| 5 | `net_intercept` | `{"action":"set","tab_id":"1773823980"}` | `BAD_ARGS` | `BAD_ARGS: net_intercept action set requires 1 to 50 rules` | 1 | `030-net_intercept_set_without_rules.json` | PASS |
| 6 | `net_intercept` | `{"action":"set","tab_id":"1773823980","rules":[{"id":1,"action":"block","url_regex":"/api/json"}]}` | `BAD_ARGS` | `BAD_ARGS: net_intercept rule id must be a non-empty string of at most 64 characters {"rule_id":""}` | 1 | `031-net_intercept_rejects_numeric_rule_id.json` | PASS |
| 7 | `net_intercept` | `{"action":"set","tab_id":"1773823980","rules":[{"id":"dup","action":"block","url_regex":"/api/json"},{"id":"dup","action":"block","url_regex":"/api/slow"}]}` | `BAD_ARGS` | `BAD_ARGS: net_intercept rule ids must be unique {"rule_id":"dup"}` | 1 | `032-net_intercept_rejects_duplicate_rule_ids.json` | PASS |
| 8 | `net_intercept` | `{"action":"set","tab_id":"1773823980","rules":[{"id":"bad-regex","action":"block","url_regex":"["}]}` | `BAD_ARGS` | `BAD_ARGS: rule url_regex is not a valid regular expression: Invalid regular expression: /[/: Unterminated character class (trace trace-1794649884577-000499) {"rule_id":"bad-regex"}` | 4 | `033-net_intercept_rejects_invalid_regex.json` | PASS |
| 9 | `download_wait` | `{"timeout_ms":100}` | `TIMEOUT` | `TIMEOUT: no download finished within 100 ms (trace trace-1794649884763-000537) {"waited_ms":105}` | 110 | `052-download_wait_no_new_download_times_out.json` | PASS |
| 10 | `download_wait` | `{"id":"no-such-download","timeout_ms":5000}` | `BAD_ARGS` | `BAD_ARGS: args.id must be a non-negative integer download id (trace trace-1794649885170-000547) {"id":"no-such-download"}` | 3 | `057-download_wait_rejects_non-decimal_id.json` | PASS |
| 11 | `download_wait` | `{"id":"999999999","timeout_ms":100}` | `TIMEOUT` | `TIMEOUT: no download finished within 100 ms (trace trace-1794649885177-000549) {"id":999999999,"waited_ms":110}` | 125 | `058-download_wait_unknown_decimal_id_times_out.json` | PASS |
| 12 | `download_list` | `{"limit":201}` | `BAD_ARGS` | `BAD_ARGS: download_list argument limit must be an integer from 1 to 200` | 2 | `060-download_list_rejects_limit_201.json` | PASS |
| 13 | `upload_file` | `{"target":"#file","files":[],"tab_id":"1773823980"}` | `BAD_ARGS` | `BAD_ARGS: upload_file requires 1 to 10 absolute file paths` | 1 | `064-upload_file_rejects_empty_files.json` | PASS |
| 14 | `upload_file` | `{"target":"#file","files":["C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt","C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt","C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt","C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt","C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt","C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt","C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt","C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt","C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt","C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt","C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt"],"tab_id":"1773823980"}` | `BAD_ARGS` | `BAD_ARGS: upload_file requires 1 to 10 absolute file paths` | 3 | `065-upload_file_rejects_eleven_files.json` | PASS |
| 15 | `upload_file` | `{"target":"#net-out","files":["C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt"],"tab_id":"1773823980"}` | `BAD_ARGS` | `BAD_ARGS: the target is not a file input (trace trace-1794649885382-000559) {"input_type":null,"reason":"not_a_file_input","tag":"p"}` | 7 | `066-upload_file_rejects_non-file_target.json` | PASS |
| 16 | `upload_file` | `{"target":"#file","files":["C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt.missing"],"tab_id":"1773823980"}` | `BAD_ARGS` | `BAD_ARGS: the file could not be read: C:\Users\ADMINI~1\AppData\Local\Temp\corvus-m2b-live\upload.txt.missing (trace trace-1794649885391-000561) {"path":"C:\\Users\\ADMINI~1\\AppData\\Local\\Temp\\corvus-m2b-live\\upload.txt.missing"}` | 7 | `067-upload_file_rejects_missing_path.json` | PASS |
| 17 | `upload_file` | `{"target":"#file","files":["relative.txt"],"tab_id":"1773823980"}` | `BAD_ARGS` | `BAD_ARGS: upload_file files must be absolute paths {"path":"relative.txt"}` | 1 | `068-upload_file_rejects_relative_path.json` | PASS |
| 18 | `browser_wait` | `{"selector":"#never","timeout_ms":100,"tab_id":"1773823980"}` | `TIMEOUT` | `TIMEOUT: browser_wait timed out after 107 ms (trace trace-1794649885826-000601) {"waited_ms":107}` | 114 | `088-browser_wait_missing_selector_times_out.json` | PASS |
| 19 | `page_text` | `{"browser_id":"bogus-browser-id","tab_id":"1773823980"}` | `TRANSPORT_DOWN` | `TRANSPORT_DOWN: browser bogus-browser-id is not connected {"browser_id":"bogus-browser-id"}` | 3 | `099-page_text_unknown_browser_id.json` | PASS |
| 20 | `tab_switch` | `{"tab_id":"tab-999"}` | `NOT_FOUND` | `NOT_FOUND: tab tab-999 does not exist (trace trace-1794649886538-000624) {"tab_id":"tab-999"}` | 3 | `100-tab_switch_unknown_tab_id_is_NOT_FOUND.json` | PASS |
| 21 | `tab_switch` | `{"tab_id":999}` | `BAD_ARGS` | `BAD_ARGS: tab_switch requires a non-empty string tab_id argument` | 3 | `101-tab_switch_numeric_tab_id_is_BAD_ARGS.json` | PASS |
| 22 | `corvus_missing_tool` | `{}` | `BAD_ARGS` | `BAD_ARGS: unknown tool corvus_missing_tool {"known":["browser_navigate","page_snapshot","browser_click","browser_type","page_text","page_html","page_find","browser_hover","browser_press","browser_scroll","browser_select","browser_eval","browser_wait","browser_history","tab_list","tab_new","tab_close","tab_switch","browser_list","net_capture","net_log","net_clear","net_intercept","download_list","download_wait","upload_file"],"tool":"corvus_missing_tool"}` | 1 | `102-unknown_tool_is_BAD_ARGS.json` | PASS |

### 4.1 Error code counts

| code | count | scenarios | retryable (taxonomy) |
|---|---|---|---|
| `BAD_ARGS` | 17 | net_capture action; net_log limit 0/1001 and inverted status range; net_intercept missing rules, numeric rule id, duplicate rule ids, invalid regex; download_wait non-decimal id; download_list limit 201; upload_file empty/11 files, non-file target, missing path, relative path; tab_switch numeric tab id; unknown tool | no |
| `TIMEOUT` | 3 | download_wait with no new download; download_wait unknown decimal id; browser_wait missing selector | yes |
| `TRANSPORT_DOWN` | 1 | page_text with unknown `browser_id` | yes |
| `NOT_FOUND` | 1 | tab_switch unknown string `tab-999` | yes |
| `NOT_INTERACTABLE` | 0 | not triggered in this matrix | yes |
| `NAVIGATION_LOST` | 0 | not triggered; immediate post-history DOM calls all passed | yes |
| `CAPABILITY_UNAVAILABLE` | 0 | extension exposes `dom` and `debugger` | no |
| `BROWSER_GONE` | 0 | not triggered | no |
| `INTERNAL` | 0 | not triggered | no |

Observed details included `details.rule_id` (invalid regex, numeric/duplicate id), `details.browser_id`, `details.tab_id`, `details.reason: "not_a_file_input"`, `details.path`, `details.waited_ms`, `details.id`, and `details.known`/`details.tool` for the unknown tool. `BROWSER_GONE` and `INTERNAL` remain unobserved, not failed.

## 5. Latency notes

Client-side latency per happy-path checkpoint. Budgets are the M2b playbook budgets; `browser_wait` uses its requested wait budget and `download_wait` uses its timeout budget.

| family | runs | min | median | p95 | max | budget | verdict |
|---|---|---|---|---|---|---|---|
| `browser_list` | 1 | 3 | 3 | 3 | 3 | 1000 | PASS |
| `tab_*` | 6 | 4 | 26 | 214 | 214 | 2000 | PASS |
| `browser_navigate` | 4 | 181 | 183 | 193 | 193 | 2000 | PASS |
| `page_snapshot` | 4 | 8 | 15 | 31 | 31 | 1000 | PASS |
| `page_text` | 12 | 4 | 9 | 32 | 32 | 1000 | PASS |
| `page_html` | 2 | 8 | 14 | 19 | 19 | 1000 | PASS |
| `page_find` | 4 | 4 | 5 | 20 | 20 | 1000 | PASS |
| `net_capture` | 3 | 3 | 8 | 26 | 26 | 1000 | PASS |
| `browser_click` | 8 | 4 | 11 | 52 | 52 | 1000 | PASS |
| `browser_wait` | 6 | 3 | 9 | 678 | 678 | 15000 | PASS |
| `net_log/net_clear` | 10 | 2 | 3 | 6 | 6 | 1000 | PASS |
| `net_intercept` | 3 | 4 | 4 | 23 | 23 | 1000 | PASS |
| `browser_eval` | 5 | 3 | 5 | 13 | 13 | 30000 | PASS |
| `download_list` | 3 | 3 | 4 | 9 | 9 | 2000 | PASS |
| `download_wait` | 2 | 9 | 113 | 217 | 217 | 35000 | PASS |
| `upload_file` | 1 | 19 | 19 | 19 | 19 | 2000 | PASS |
| `browser_hover` | 1 | 8 | 8 | 8 | 8 | 1000 | PASS |
| `browser_press` | 1 | 6 | 6 | 6 | 6 | 1000 | PASS |
| `browser_scroll` | 2 | 7 | 8 | 8 | 8 | 1000 | PASS |
| `browser_select` | 1 | 5 | 5 | 5 | 5 | 1000 | PASS |
| `browser_type` | 1 | 22 | 22 | 22 | 22 | 1000 | PASS |
| `browser_history` | 3 | 61 | 64 | 104 | 104 | 2000 | PASS |

Notes:

- The slowest call was `browser_wait` at 678 ms, waiting for the deliberate 600 ms `/api/slow` response; this is wait time, not tool overhead.
- `download_wait` took 217 ms after the download click, including browser download completion; the explicit-id follow-up took 9 ms.
- `tab_new` took 214 ms, the slowest tab operation; all other tab calls were 3–52 ms.
- `browser_navigate` ranged 181–193 ms; `page_snapshot` ranged 8–31 ms; every interactive call stayed below 52 ms except waits/downloads.
- All 22 negative cases completed in 1–125 ms.

## 6. Skipped scenarios

| scenario | reason | coverage elsewhere |
|---|---|---|
| Omitted `browser_id` with two live browsers must return BAD_ARGS `multiple_browsers` | Only one browser (`edge:33628f01180d3d48`) was connected; a second live browser was not available. Creating a second real browser session was out of scope for this single-tab acceptance run. | The 108-check conformance suite covers this with two mock extensions `mock:conformance` and `mock:b`. Live explicit `browser_id` routing and live unknown-`browser_id` TRANSPORT_DOWN both passed. |
| Extension disabled then `page_text` must return TRANSPORT_DOWN | Disabling the live extension would disrupt the running Corvus service and the client browser session; QA was instructed not to stop or repurpose the service. | Daemon transport-down behavior is covered by conformance/end-to-end tests; not re-proven live in this run. |

These are coverage gaps, not failures. `passed=105 failed=0` remains the result of the executed matrix.

## 7. Defects

**No product defects found.**

Harness corrections made during execution (not product defects):

- The first driver attempt used a checkpoint label with `/api/` directly in the evidence filename, causing an `ENOENT` while saving evidence. The evidence filename was sanitized; no MCP call had failed.
- One assertion initially expected the mock response marker to be `mock`; the fixture correctly stores the response as `mock:99` (`source:value`). The assertion was corrected; the product result was correct.

No retry or sleep-based pass was used. The bfcache checks immediately followed `browser_history back`, `forward` and `reload` with `page_text` and `page_snapshot` on the same tab; all succeeded.

## 8. Residual risks

- Multi-browser live ambiguity is not exercised (section 6); the conformance suite covers the routing branch with mocks.
- Extension-disabled TRANSPORT_DOWN is not exercised live; service continuity was preserved.
- The `net_intercept` runtime Fetch-action failure path described in the M2b known risks was not observed. If it occurs, the extension logs it and continues the request; the original `set` response cannot surface it (S3 observability limitation).
- `net_log` uses a 2000-entry per-tab ring; this run reached only 4 entries, so eviction behavior was not exercised live.
- `download_wait` without an id uses the 5 s extension recency window. Seven older downloads existed before the click; the wait correctly returned the new download id `8` rather than an old one.
- Download filename uniquification was not observed: the actual file was `corvus-report.txt`. A future run may see `corvus-report (1).txt`; the report should then record the actual filename.
- The live daemon was an already-running service on default ports 7330/7331; this was not a cold-boot run. The QA-owned fixture server ran on 8812.
- Evidence is retained under the temporary directory above, outside the repository. Copy it with this report if it must be archived.

## 9. Cleanup and sign-off

| check | result |
|---|---|
| Own tab closed | PASS — `tab_close` returned `{"closed":true,"tab_id":"1773823980"}` |
| Previous active tab restored | PASS — final `tab_list` shows 2 tabs, `1773823880` active |
| Intercept rules cleared | PASS — tab-scoped `net_intercept clear` returned `rules:[]` |
| Capture stopped | PASS — `net_capture stop` returned `capturing:false` |
| Live Corvus service | left running on 7330/7331; not restarted or stopped |
| QA fixture server on 8812 | stopped after the run |
| Evidence files | 105 checkpoint JSON files + `summary.json` + `report-data.json` |

| role | name | date | verdict |
|---|---|---|---|
| QA operator | qa | 2026-11-14 | PASS |
| Team Lead |  |  |  |

The report is based on the live run above; it is not a template copy. The 26-tool conformance result for this tree remains `108 passed, 0 failed, 0 skipped`.
