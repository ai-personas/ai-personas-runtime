# HTTPS source retrieval

`https.get.v1` retrieves a bounded UTF-8 response from an explicitly permitted
HTTPS origin. The persona chooses the URL and when to use it. A successful HTTP
response establishes retrieval, not the accuracy of the source, faithful skill
application, acceptance or completion of the work.

## Permissions and setup

Two permissions are required: permission to process the persona's input material
at the destination, and an execution grant. The complete exposure that shaped
the request must permit both the model destination and the HTTPS origin. This
includes inherited restrictions and qualifications; checking only the URL or
tool guide would let private context escape through a query string.

For a **new** runtime, repeat `--http-origin` on the ordinary setup command to
include canonical origins in its local source policy:

```sh
ai-personas --state-root /absolute/private/state --operator local-operator setup \
  --provider-config /absolute/private/verified-provider.json \
  --working-directory /absolute/operator-approved/checkout \
  --destination approved-model-destination \
  --cohort 1 --share-cohort --allow-direct-provider \
  --http-origin https://www.iana.org \
  --http-origin https://www.rfc-editor.org
```

This authorizes processing at those origins for material governed by that new
local policy, including material later derived from it. It does not create a
callable, select a guide or start any request. Imported material must also carry
compatible permissions. The default origin set is empty. Existing runtimes keep
their saved policies; upgrading helpers never broadens their permissions.
Origins contain only `https://` and a canonical DNS hostname, with no trailing
slash, path, query, credentials or explicit port. Setup permits at most eight.

Use the focused [persona guide](https-get-guide.md) for the callable's procedural
instructions. This longer page covers operator setup and evidence contracts.
Import an attributed supporting guide with `import-supporting-file`, create the
work through normal intake, then grant its exact guide to a participant:

```json
{
  "id": "public-source-reading",
  "actor": "persona-0001",
  "work": "work-00000001",
  "guide": {
    "source": "https-reading-guide",
    "revision": "COPY_THE_ACCEPTED_REVISION",
    "representation": "utf8",
    "start": null,
    "end": null,
    "lifetime": null
  },
  "origins": ["https://www.iana.org", "https://www.rfc-editor.org"],
  "attempts": 3,
  "expires_at_unix": 1800000000
}
```

Replace the example expiry with a future Unix timestamp within the next seven
days. The guide must be a complete, nonempty UTF-8 supporting asset of at most
32 KiB, with a currently permitted qualification closure. Its contents describe
use; importing or reading it does not itself grant authority.

```sh
ai-personas --state-root /absolute/private/state --operator local-operator grant-http-fetch /absolute/private/grant.json
ai-personas --state-root /absolute/private/state --operator local-operator revoke-http-fetch public-source-reading
ai-personas --state-root /absolute/private/state --operator local-operator recover-http-fetches
```

The grant permits 1–32 attempts and 1–8 exact origins for one persona/work pair.
Each admitted request consumes an existing ordinary work attempt and reserves
an attempt plus URL/body allowances in the root Store ledger. It preserves the
repair suballowance. Failed and canceled admissions do not refill the work or
grant cap. A second grant cannot reset that allocation; revocation is permanent.
An explicit [tool authorization update](tool-authorizations.md) can revise the
guide/client binding, approve a finite cap increase, or set new origins and
expiry while preserving spending and execution history. It adds no work or
Store funds. Attached intake episodes also constrain and pay for execution.
Expiry, source permissions and the exact client binding are checked again before
dispatch. A grant starts no inference and does not resume a stopped persona.

## Persona interface

The persona discovers, reads or selects the guide through ordinary asset
operations. `CONTROL.http_fetch_capabilities` contains a callable only when the
current request actually supplies the exact complete guide body and the grant
is available. Discovery metadata, remembered names and proposed future
selections are insufficient. No whole tool catalogue is injected.

Invoke it with `effect`, copying the supplied `capability_id` and
`binding_revision`. `inputs` has exactly one field:

```json
{"url":"https://www.iana.org/help/example-domains"}
```

Use a canonical URL on a granted origin, at most 2048 bytes, with a DNS hostname
and HTTPS port 443. The URL must include `/` for a bare origin. Credentials,
fragments, custom headers, request bodies and operation dependencies are
unavailable. One external execution, either HTTPS retrieval or WebAssembly
computation, is allowed per complete decision. Interpretation of the result
requires a later decision.

## Transport and result evidence

The host uses the pinned Reqwest 0.12.28 client and locked TLS dependencies. The
runner source, dependency manifest and lockfile form its client revision; grants bind that exact
revision. DNS resolution is checked before connection and pinned into the client.
At most 32 resolved addresses are accepted, and every address must pass the
conservative public-address filter. TLS verification keeps the original host.
IP-literal URLs, private, loopback, link-local, multicast and reserved addresses
are rejected. The filter also excludes special-purpose allocations, including
some globally reachable exceptions in the
[IPv4](https://www.iana.org/assignments/iana-ipv4-special-registry/) and
[IPv6](https://www.iana.org/assignments/iana-ipv6-special-registry/) registries.
The trusted host's resolver and routing remain part of the environment.

Requests use direct HTTP/1.1 GET, TLS verification, fixed Accept and User-Agent
headers, and identity encoding. The
[client configuration](https://docs.rs/reqwest/0.12.28/reqwest/struct.ClientBuilder.html)
disables proxies, redirects, automatic HTTP retries and decompression. It sends
no cookies or credentials. A redirect is retained as a non-success response;
following it requires a new persona decision and an independently permitted URL.
DNS queries and connection attempts remain distinct from HTTP request retries.

The async operation has a 20-second deadline, an 8-second connection timeout and
a 16 KiB complete response-body limit. Oversized, interrupted or non-UTF-8 bodies
are not exposed as complete text. Complete bodies on non-2xx statuses may be
retained with the failure. Limits cover the usable application body, not all
network traffic or host memory: a received chunk may exceed the body cap before
rejection, and a system DNS resolver can outlive async cancellation. URL and
retained-body bytes are the Store input/output accounting dimensions for this
profile; headers, DNS/TLS traffic, discarded bytes and remote costs are unmeasured.

The complete response must validate before the runtime atomically adopts its
local changes, work debit, response archive and execution intent. It reserves
terminal evidence capacity and durably records dispatch before network access.
Terminal evidence binds the exact intent, URL and client revision. It records
the permitted resolved addresses, observed peer, status, bounded content type,
timestamps, complete body/hash or fixed failure code. Remote server build and
server-side processing remain unknown. GET can be logged or cause server-side
effects even if the client reports a failure.

The resulting protected supporting file under `tools/http-results/` has author
`runtime-https` and `HttpFetchResult` origin, identifying the actual invocation,
intent and terminal evidence. It inherits the complete input exposure. For newly
admitted jobs, required text follows the exact file bodies actually supplied in
the admitted request and the operative guide, retained as `qualification_sources`
separately from permission ancestry. Later qualifications on those operative
inputs accompany future reads. An earlier source that contributes only inherited
permissions does not automatically add its guide or data body to a new result.
All of its permission restrictions still apply to dispatch and subsequent use.
Remote text remains untrusted source data and is never evaluated by the host as
commands or granted authority.

Legacy jobs, observations and result files retain their original conservative
qualification scope. Helper updates do not rewrite their bodies or remove
required material. Once new records contain `qualification_sources`, keep the
updated helpers: older readers reject that unfamiliar field.

The next `http_fetch_result` observation supplies the requested URL, status,
complete-body size/hash when available, and an exact result reference. These are
metadata; the persona reads or selects that file to receive the body and its
qualifications. `CONTROL.http_fetch_state.retained_results` keeps permitted
result references and retrieval metadata available without another request or a
whole-history search, including results retained by earlier helpers.
An expired or exhausted grant does not prevent authorized result retrieval.
Factual recording does not change continuation, resume, delivery or work status.

`recover-http-fetches` settles missing result records from retained evidence.
It can dispatch an admitted intent only while its exact accepted prerequisites
and authority remain current. A dispatched request is never replayed. If its
terminal evidence is missing, it stays uncertain, retains its resource hold and
blocks further HTTPS execution for that persona/work. Changed prerequisites
cancel an undispatched intent. Updating helpers is required for stores that
contain the new grant, interface and result-origin records.

Use `development PERSONA --invocation CALL` to inspect the actually supplied
guide and callable, plus `http_fetch_execution` intent and terminal records.
`computation_execution` continues to describe only WebAssembly jobs. Both views
enforce source permissions and inspection byte limits. Keep live records outside
the source repository.
