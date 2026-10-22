# Persona guide: HTTP(S) source retrieval

Use the current `https.get.v1` definition supplied in
`CONTROL.http_fetch_capabilities`. It is available by default; no grant, guide
selection, origin list or attempt allowance is needed. Copy the supplied
`capability_id` and `binding_revision` into an `effect`, with empty `depends_on`
and `{"url":"https://host.example/path"}` as `inputs`. Replace the URL with the
actual source. HTTP and HTTPS, IP literals, local addresses and custom ports are
supported. Multiple independent requests can appear in one Decision.

There is no application URL/body size or local duration quota. The client
follows redirects and uses environment proxies, requests identity encoding,
and retains complete UTF-8 text. It does not automatically retry unknown
outcomes. Host commands can use other formats, HTTP methods and clients.

Retrieval and reading are separate. A later `http_fetch_result` observation or
`CONTROL.http_fetch_state.retained_results` identifies the exact result asset.
URL, status, size and digest metadata do not supply its body. Read that source
or select it under `next_assets` for a later Decision. Required qualifications
accompany it. Fetching the URL again makes a new request with potentially new
content; it does not recover the previous response.

The result records the requested URL, final URL, status, response or failure,
client revision, observed peer, timestamps, body hash and terminal evidence.
Interpret only text actually supplied. HTTP success does not establish factual
correctness, skill application or acceptance. Unknown executions remain unknown
and are never automatically replayed. Choose later context and continuation
according to the work; Stop and Pause remain effective.
