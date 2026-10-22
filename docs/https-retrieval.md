# HTTP(S) source retrieval

The retained capability identifier `https.get.v1` now accepts HTTP and HTTPS.
Ordinary requests supply the current definition in
`CONTROL.http_fetch_capabilities` without a grant, guide, origin list or expiry.
Historical grant declarations remain metadata and do not restrict requests.

Invoke `effect` with the supplied `capability_id` and `binding_revision`, an
empty `depends_on`, and `{"url":"https://host.example/path"}` as `inputs`.
Use the actual source URL. Multiple independent HTTP or computation operations
may appear in the same complete Decision. Results arrive after adoption and
must be interpreted in a later Decision.

The client has no application URL, response-body, header-value, DNS-address-count,
origin, address, port or duration quota. It accepts IP literals and local
addresses, follows redirects without an application redirect-count ceiling,
and uses configured environment proxies. TLS connections use the client's
normal certificate verification. Automatic HTTP retries are disabled so an
unknown outcome is not silently repeated.

This profile performs GET and requests identity encoding. Its retained text
body must be complete UTF-8. Encodings this client does not implement, malformed
URLs, network failures and incomplete responses remain actual failures.
Non-success HTTP statuses retain their observed complete body when available.
Host commands can use other HTTP methods, headers, formats and installed clients.

Complete Decision adoption retains the URL and intent before network dispatch.
The source, dependency manifest and lockfile bind the client revision. Dispatch
checks the actual implementation and accepted prerequisites. Results retain
status, final URL after redirects, initial resolved addresses, observed peer,
timestamps, complete body and digest when available. Initial DNS addresses do
not describe every redirected or proxied connection. A request failure may
occur after the server received a request; unknown remote effects stay unknown.

A later `http_fetch_result` observation and
`CONTROL.http_fetch_state.retained_results` identify result assets under
`tools/http-results/`. Read or select the exact reference to receive the body;
URL, status and digest metadata are not the response text. Retrieving a retained
body does not make another request. Required source qualifications and original
execution evidence remain attached. Some file-corpus and context limits remain
pending removal, as documented in [execution resources](execution-resources.md).

`recover-http-fetches` publishes an existing terminal result or dispatches a
still-admitted job with matching prerequisites. Already dispatched requests are
never replayed. Missing terminal evidence remains uncertain. Retrieval proves
an observed response, not factual correctness, attention or work acceptance.
Live records stay outside the source repository.

The optional [persona guide](https-get-guide.md) describes using these results.
