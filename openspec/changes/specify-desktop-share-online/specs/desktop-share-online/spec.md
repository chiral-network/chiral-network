## ADDED Requirements

### Requirement: Sharing is an explicit, ordered wizard

The native desktop SHALL offer sharing only through an explicit Share wizard whose six steps run in this order: (1) choose an installed model and serving settings, (2) provider identity and wallet, (3) reachability, (4) qualification, (5) pricing, (6) publish. Every new share is first published free, in the launch window that *New shares start free in a launch window* defines. In step 4 the user either approves the budget of the qualification that runs automatically while the share serves (*Qualification runs automatically in the share's first quiet hour*), or shares without qualification. In step 5 the user chooses **Auto-price**, which is selected by default (*Auto-price sets the price from launch reputation*), or manual pricing (*Manual pricing applies when Auto-price is off*), and the free allowance (*Free offers carry a signed free allowance*). An embedding share needs no speed qualification. A step SHALL be enabled only when every earlier step is complete for the current choices. Changing an earlier choice SHALL invalidate every later result that depends on it, including reachability verification, a qualification budget approval, qualification results, pricing and disclosure acceptance. The wizard SHALL NOT start a provider daemon, spend funds, publish an offer or change a published price except through the explicit actions this capability defines. The automatic qualification run, the promotion of its result and Auto-price's price changes are actions the user authorizes in the wizard; the user can cancel or stop them, or turn Auto-price off, on the Share dashboard.

Local use SHALL keep working without a provider profile, wallet or network. Opening the Share page SHALL NOT contact the chain, a catalog, a relay or a provider until the user selects a provider profile. Sharing SHALL NOT start automatically, including when the desktop starts after a share was active when it last exited. In the browser preview every share action SHALL fail with an explanatory error, and the GUI SHALL NOT fabricate wallets, balances, reachability results, qualification results, offers or a daemon.

#### Scenario: Later steps cannot be reached early

- **GIVEN** the user has completed only the first step
- **WHEN** they try to open the pricing or publish step
- **THEN** the step is unavailable
- **AND** no daemon has been started, no funds have been spent and no offer has been published

#### Scenario: Changing a serving setting invalidates qualification

- **GIVEN** a qualification budget approved, or a passing qualification, for maximum concurrency 2
- **WHEN** the user changes the maximum concurrency to 3 in the first step
- **THEN** the budget approval, any qualification result, pricing and disclosure acceptance no longer allow a qualification run or publishing
- **AND** the wizard requires a new budget approval, and a new qualification run, for the new settings

#### Scenario: A previous share does not restart by itself

- **GIVEN** a share was active when the desktop last exited
- **WHEN** the desktop starts again
- **THEN** no provider daemon is started and no offer is published
- **AND** the Share page shows the stopped share, the liabilities last observed with their observation time, and an explicit *Share again* action

#### Scenario: Browser preview

- **WHEN** a share action is invoked outside the native runtime
- **THEN** it fails with an explanatory error
- **AND** no wallet, balance, offer, qualification result or daemon state is shown as real

### Requirement: The Share page is reachable from navigation

The native shell SHALL add the hash route `#/share` and a navigation item **Share** with a label, a short label and an icon. The item SHALL be placed after Local server when that item exists, and otherwise after My models, and before the Network section, in the navigation order that `desktop-local-workbench` defines. It SHALL expose `aria-current` when active, and the window title on the page SHALL be `Share | Chiral Network`. The item SHALL be added together with the native share commands this capability defines, and SHALL NOT appear in a build that lacks them. In the browser preview the Share page SHALL show the preview notice and its actions SHALL fail as *Sharing is an explicit, ordered wizard* requires.

#### Scenario: Share route

- **GIVEN** a native build that includes the share commands
- **WHEN** the user opens `#/share`
- **THEN** the Share page is shown, its navigation item is marked with `aria-current`, and the window title is `Share | Chiral Network`
- **AND** the item appears after Local server (or after My models when Local server is absent) and before the Network section

### Requirement: The shared model and serving settings are exact and verified

The first step SHALL offer only models that `desktop-model-acquisition` (*My models lists installations and their verification state*) reports as installed and verified, with a verified pinned runtime for this platform and backend. The share's model identity, context length and maximum output SHALL be exactly those of the installed model's ModelSpec; the desktop SHALL NOT edit an installed ModelSpec or artifact inventory to change them. The user SHALL choose the remaining serving settings explicitly: maximum concurrency (1–256), devices and GPU offload, split mode and weights, KV cache types, flash attention, batch and micro-batch sizes and memory reserve, each within the native provider-configuration bounds.

The desktop SHALL refuse settings that fail the fit check — total context of per-slot context × maximum concurrency plus weights within physical memory minus headroom of at least 4,096 MiB for unified memory or 256 MiB per dedicated device — and SHALL NOT silently reduce them. A pre-load fit result SHALL be labeled an estimate; actual fit is checked after loading. For a generation share that requires qualification, the desktop SHALL require a context length of at least 4,096 tokens, a maximum output of at least 256 tokens, and a GPU backend with a qualifying physical-memory counter (CUDA dedicated memory on Linux or Windows, or Metal unified memory on Apple Silicon). Otherwise it SHALL explain, before any qualification budget is requested, that this model or machine cannot be qualified for paid generation and can be shared only free: Auto-price never raises its price and manual pricing cannot set a nonzero one. A free share is not subject to these qualification requirements; it still uses the installed ModelSpec's limits and SHALL pass the fit check.

Sharing SHALL hand the engine to the provider daemon. The model SHALL NOT be loaded twice, and the daemon SHALL launch its own managed runtime with the share's settings; it SHALL NOT use the desktop's engine as an external endpoint. If the chosen model is loaded in the local engine, the wizard SHALL state that the local instance will be ejected and SHALL eject it only after the user confirms.

Before an installation is shared for the first time, the wizard SHALL disclose that downloaded GGUF files, including embedded chat templates, are untrusted inputs to an engine that is not an operating-system sandbox and runs as the user. A file exploiting an engine or template-parser bug could therefore read the keys and tokens under the app's `network/` and `local-server/` directories, including the provider signing key. A repository counts as vetted only as `desktop-local-runtime` (*Loading uses explicit settings within the installed limits*) defines vetting, which currently treats every repository as unvetted; otherwise every repository is treated as unvetted for this disclosure.

#### Scenario: Settings that do not fit are refused

- **GIVEN** a model whose weights and per-slot context fit at concurrency 1 but not at concurrency 4 with the required headroom
- **WHEN** the user selects concurrency 4
- **THEN** the wizard refuses the setting and explains the fit failure
- **AND** it does not lower the concurrency or context on the user's behalf

#### Scenario: A model that cannot qualify is identified before spending

- **WHEN** the user chooses a generation model whose installed context is 2,048 tokens, or a CPU or Vulkan backend
- **THEN** the wizard states that this share cannot be qualified for paid generation and can only be shared free
- **AND** it does not request a qualification budget

#### Scenario: The local instance is handed over only after confirmation

- **GIVEN** the chosen model is loaded in the local engine for chat
- **WHEN** the user continues the wizard
- **THEN** the wizard asks to eject the local instance before the provider daemon loads the model
- **AND** declining leaves the local instance loaded and the share not started

#### Scenario: Untrusted-artifact risk disclosed before the first share

- **WHEN** the user chooses an installation that has not been shared before, from a repository not marked as vetted
- **THEN** the wizard discloses that the engine is not a sandbox and that a malicious file could read the keys and tokens under `network/` and `local-server/`
- **AND** the wizard continues only after the user acknowledges it

### Requirement: Provider identity and wallet share one data_dir journal

The second step SHALL require an explicitly selected provider profile: a strict JSON document, selected by path, that contains no secrets and rejects unknown fields. Its fields SHALL be exactly:

- `network`: the consumer-profile network object, with the consumer profile's constraints: `identity` (`chain_id`, `genesis_hash`, `deployment_id`), `rpc_url`, and the nullable `rpc_bearer_file` (a path to a protected file), `faucet_url`, `explorer_url` and `policy_url`;
- `platform_wallet`: the canonical wallet to which the platform fee is owed;
- `dht_bootstrap`: a nonempty list of peer multiaddresses, each ending in `/p2p/<peer ID>`;
- `relays`: a list of 1–32 relay multiaddresses, each ending in `/p2p/<relay peer ID>`. Every share uses libp2p and is verified through a relay (*Reachability is verified through a relay before publishing*), so the wizard SHALL refuse a profile whose list is empty or has more than 32 entries;
- `version_policy_file`: a nullable path to an authority-signed version policy. When it is null, `network.policy_url` SHALL be set, and the desktop SHALL fetch the policy from it, verify it natively and pass a protected copy of the verified policy to the daemon;
- `catalog_url`: the nullable origin of the catalog (a relay origin, as in the consumer profile) from which Auto-price and the Share dashboard read this provider's launch reputation and payment-gated reputation. It SHALL satisfy the consumer profile's outbound-origin rules for `catalog_url`. Before using it, the desktop SHALL verify that the catalog reports the profile's complete network identity, and SHALL refuse a catalog that reports another. When it is null, Auto-price SHALL be unavailable for the share, with that reason, and the dashboard SHALL state that no launch reputation is read.

Paths SHALL resolve relative to the profile file. The profile SHALL NOT contain a `development` field or any other setting that enables development behavior. The desktop SHALL NOT invent the profile's values or accept them from discovered peers. A provider profile shipped with the desktop MAY name the project's catalog and MAY list the serving-capable testnet relays the project operates; it is selected explicitly like any other profile, and the user MAY use a profile that also or instead lists relays that anyone else operates. The desktop SHALL use only the relays the selected profile lists. Relays charge no fee and are not a party to any agreement: the wizard SHALL NOT present a relay fee or add any payment, agreement or term for a relay. When a consumer profile is loaded, the provider profile's complete network identity (chain ID, genesis hash and deployment ID) SHALL equal the consumer profile's; otherwise the wizard SHALL refuse the provider profile.

The user SHALL choose the provider wallet: the loaded consumer wallet, a newly generated native wallet, or an imported protected key file. Every process the desktop runs that signs with the same wallet on the same network SHALL use the same `data_dir` base, so consumer funding and provider fee remittance by that wallet share one coordinated nonce journal. A new deployment's `data_dir` base SHALL be the loaded consumer profile's `data_dir` when a consumer profile is loaded, and `<app data>/network/` otherwise; an existing deployment SHALL always use the base recorded for it. The desktop SHALL natively record, per network identity, the base whose journal each wallet that signs for a share or a qualification run uses (the provider wallet and the paying wallet). It SHALL refuse to load any consumer profile, including a first load and a load after sharing stops, whose wallet is a recorded wallet for that network but whose `data_dir` differs from that wallet's recorded base. It SHALL also refuse to start a daemon or a qualification run that would use a recorded wallet under a different base. Each refusal SHALL explain that one wallet must keep one nonce journal. The desktop SHALL NOT copy a wallet key or journal into another `data_dir`. Before importing a key, the wizard SHALL warn that a key still active in another installation with an independent journal must not be imported.

A provider deployment is a persisted provider DHT identity file together with the state the daemon keeps for that identity under `<base>/<network-hash>/<provider-wallet>/<peer-id>/`: its ledger, lock, acknowledgements and offer revision. The desktop SHALL keep one published deployment per network identity and provider wallet, and SHALL reuse it for every share made with that wallet on that network, whatever model, artifact, serving profile or prices the share uses. The calibration deployment of *Paid generation shares require in-app qualification within an approved budget* is a separate deployment with its own identity. Each provider identity file SHALL be distinct from any consumer DHT identity file. No consumer node SHALL ever run with a provider's peer ID, because its empty manifest replies would withdraw the provider's offers. An identity SHALL be created once and then reused. The desktop SHALL NOT replace or delete the provider identity, ledger, offer revision, acknowledgements, journals or keys to clear an error, because a new peer ID is a different deployment, not recovery of the old one.

A deployment can hold accounts with nonzero available or reserved balances whose agreed model the share's model does not accept for execution under the daemon's execution-compatibility rule for agreed models. Before publishing a share from such a deployment, the wizard SHALL list those accounts with their exact available and reserved balances and their total. It SHALL state that they remain unusable until this machine shares, from this deployment, a model that serves them, and SHALL require the user's acknowledgement. Those accounts SHALL be preserved.

#### Scenario: One wallet acts as consumer and provider

- **GIVEN** the user chooses the loaded consumer wallet as the provider wallet
- **WHEN** the desktop generates the provider daemon configuration
- **THEN** the daemon's `data_dir` is the same base directory as the consumer profile's
- **AND** no copy of the wallet key or journal is created elsewhere

#### Scenario: A share started without a consumer profile keeps its base

- **GIVEN** a share started with no consumer profile loaded, whose provider wallet uses the base `<app data>/network/`
- **WHEN** the user loads a consumer profile whose wallet is that provider wallet and whose `data_dir` is a different directory
- **THEN** loading the profile is refused with an explanation that one wallet must keep one nonce journal
- **AND** no process signs with that wallet under the other base

#### Scenario: A provider profile for another network is refused

- **GIVEN** a loaded consumer profile for one deployment ID
- **WHEN** the user selects a provider profile with a different deployment ID or genesis hash
- **THEN** the wizard refuses the provider profile and explains the mismatch

#### Scenario: A provider profile with an unlisted field is refused

- **WHEN** the user selects a provider profile that contains a field this requirement does not list, such as `development`
- **THEN** the wizard refuses the profile and names the field

#### Scenario: Relay list bounds

- **WHEN** the user selects a provider profile whose `relays` list has 33 entries or is empty
- **THEN** the wizard refuses the profile and explains that a share needs 1–32 relays

#### Scenario: A catalog for another network is refused

- **GIVEN** a provider profile whose `catalog_url` names a catalog that reports a different deployment ID
- **WHEN** the wizard verifies the catalog
- **THEN** it refuses the catalog and explains the mismatch
- **AND** Auto-price is not offered with that catalog and no figure from it is shown

#### Scenario: Sharing again reuses the deployment

- **GIVEN** a stopped share with existing prepaid accounts
- **WHEN** the user chooses *Share again*, sharing the same installation with the same serving profile and the same provider wallet
- **THEN** the daemon starts with the same peer ID, ledger and offer-revision state
- **AND** existing accounts remain servable under their accepted terms

#### Scenario: Sharing a different model lists the accounts it will not serve

- **GIVEN** a deployment whose accounts for model A hold available balance
- **WHEN** the user shares model B with the same provider wallet on the same network
- **THEN** before publishing, the wizard lists model A's accounts with their exact total liability and requires acknowledgement
- **AND** the daemon runs with the same peer ID and those accounts remain in its ledger

#### Scenario: A free share does not serve paid accounts with a serving profile

- **GIVEN** a deployment whose accounts were opened under a qualified share of model A and hold available balance
- **WHEN** the user makes a free share of model A from the same deployment
- **THEN** before publishing, the wizard lists those accounts with their exact total liability as accounts the free share will not serve, and requires acknowledgement
- **AND** those accounts remain in the ledger

### Requirement: The provider wallet holds gas for fee remittance

The second step SHALL show the provider wallet's verified-network CFX balance and SHALL state that the provider pays the gas to remit its platform-fee obligations, which the managed daemon sends automatically from the provider wallet at the aggregation thresholds. It SHALL show an estimated gas cost for one fee-remittance transaction, labeled as an estimate. It SHALL show the provider profile's faucet and explorer only when configured, and SHALL state that no faucet authority is embedded and that Core and eSpace balances are separate. The desktop SHALL NOT request faucet funds or transfer funds on the user's behalf. Publishing SHALL be unavailable while the verified balance is below the remittance estimate. This applies to every share, including a free share. For a free share the wizard SHALL also state that its own openings carry zero funding and so create no platform-fee obligation, and that the gas covers remitting any obligation the deployment holds or later incurs. Whether a free share without fee obligations may publish without gas is open question 12 of this change's design. When the balance cannot be verified, the wizard SHALL show the RPC error instead of a zero or stale balance presented as verified, and SHALL NOT treat an unverifiable balance as sufficient.

#### Scenario: Insufficient gas blocks publishing

- **GIVEN** the provider wallet's verified balance is below the estimated cost of one fee remittance
- **WHEN** the user reaches the publish step
- **THEN** publishing is unavailable with an explanation and the configured faucet or explorer, if any

#### Scenario: A free share still needs remittance gas

- **GIVEN** a free share whose deployment holds no unremitted fee obligation, and a provider wallet with a verified balance of zero
- **WHEN** the user reaches the publish step
- **THEN** publishing is unavailable until the verified balance covers the remittance estimate
- **AND** the wizard states that the free share's own openings create no platform-fee obligation

#### Scenario: Unverifiable balance

- **WHEN** the configured RPC cannot verify the provider wallet's balance
- **THEN** the wizard shows the RPC error
- **AND** it does not display the balance as verified or allow publishing on it

### Requirement: Reachability is verified through a relay before publishing

The third step SHALL offer exactly one transport: libp2p, reached directly, by hole punching or through a relay. Desktop shares are libp2p-only (book *Author decisions* item 7). The wizard SHALL NOT offer an HTTPS origin, and SHALL NOT accept an HTTP origin or endpoint of any kind for a share, including the local API server's address. It SHALL state that a provider that wants an HTTPS origin runs `chiral_daemon` with the provider guide (`docs/provider-qualification.md`) and its preflight instead. Every share's daemon configuration SHALL advertise no HTTPS origin and SHALL enable libp2p serving. Its offers are therefore the schema v2 offers that `p2p-provider-transport` defines, declaring `transports` `["libp2p"]` with a `null` `endpoint` and signed with the deployment's peer ID, and are published only on the schema v2 manifest protocol, `/chiral/inference/manifest/2`. The desktop SHALL NOT configure TLS, DNS or port forwarding, and SHALL NOT bind the provider's HTTP listener beyond loopback.

Until libp2p provider transport is implemented (`specify-p2p-provider-transport`), the reachability step SHALL be unavailable, SHALL say why, and SHALL give the same pointer to `chiral_daemon`. Every later step depends on this step, so until then no reachability check SHALL run, no qualification budget SHALL be requested and no share, free or paid, SHALL be published.

The wizard SHALL verify, through a relay and over a path that leaves this machine, that libp2p reaches this machine's provider daemon under the share's provider identity and not some other peer. The proof is the daemon's reported reachability (AutoNAT classification and held relay reservations) together with a connection through the relay, opened by a native client whose peer ID is not the provider's, whose Noise-authenticated remote peer ID equals the deployment's peer ID. The wizard SHALL show the result and the path that would be used. If verification fails, the wizard SHALL NOT publish and SHALL show the reason. Changing the provider profile, its relays or the network after a verification SHALL require a new verification. A check made before publication uses a coordination-sized relay reservation, so the wizard SHALL NOT present it as proof of serving-grade relay capacity.

The wizard SHALL run this check in the reachability step, before it requests approval of a qualification budget, and again immediately before publishing. A free or embedding share, which requests no budget, SHALL also complete the reachability-step check before its later steps are enabled. The reachability-step check SHALL run with a daemon started in reachability-check mode under the share's persisted provider DHT identity: the identity that will publish, never the calibration identity. In reachability-check mode the daemon SHALL:

- start no engine and load no model;
- publish no offer, so its manifest replies carry no offers;
- admit no inference request;
- send no chain transaction, so it performs no funding reconciliation and no fee remittance;
- hold the deployment's lock, so that no other daemon runs with that identity at the same time;
- report its reachability observation on its loopback administration status.

The check immediately before publishing SHALL run with the published deployment, started without publishing.

#### Scenario: No HTTPS origin is offered

- **GIVEN** libp2p provider transport is implemented
- **WHEN** the user opens the reachability step
- **THEN** libp2p is the only transport offered, and no field accepts an HTTPS origin or any other HTTP endpoint
- **AND** the step states that a provider that wants an HTTPS origin runs `chiral_daemon` with the provider guide and its preflight

#### Scenario: A share's offers declare only libp2p

- **WHEN** a share is published
- **THEN** each of its offers declares `transports` `["libp2p"]` with a `null` `endpoint` and is signed with the deployment's peer ID

#### Scenario: Unreachable provider is not published

- **GIVEN** the relay cannot reach this machine's provider over libp2p
- **WHEN** the user tries to publish
- **THEN** no offer is published and the failure reason is shown

#### Scenario: Reachability is checked before any qualification spending

- **WHEN** the user reaches the qualification step
- **THEN** a libp2p reachability check has already completed under the share's provider identity in reachability-check mode, and its result is shown
- **AND** no qualification budget is requested while that check is failing or unavailable

#### Scenario: Reachability-check mode publishes and spends nothing

- **WHEN** the daemon runs in reachability-check mode
- **THEN** no engine is running, its published-offer list is empty and its manifest replies carry no offers
- **AND** it sends no chain transaction and admits no inference request

#### Scenario: Nothing is shared before milestone 3

- **GIVEN** libp2p provider transport is not implemented
- **WHEN** the user opens the reachability step
- **THEN** the step is unavailable, with an explanation and the pointer to `chiral_daemon` and the provider guide for an HTTPS origin
- **AND** no reachability check runs, no qualification budget is requested and no share, free or paid, is published

### Requirement: Paid generation shares require in-app qualification within an approved budget

A generation share SHALL NOT publish a nonzero input price, output price or minimum funding unless its published deployment's model carries a serving profile from a passing qualification of the share's exact deployment profile under policy `warm-generation-v1`, which native deployment revalidation accepts and which no performance-withdrawal latch has withdrawn (a *valid qualification*). A failed, stopped or interrupted qualification run, and the calibration deployment itself, SHALL NOT make a priced share publishable. A free share skips this requirement as *Free and embedding shares publish after readiness without speed qualification* defines. The run takes place while the share is published, as *Qualification runs automatically in the share's first quiet hour* defines, and its result reaches the published deployment only as *A passing run is promoted into the published deployment in place* defines.

In the qualification step, and before any spending, the wizard SHALL collect the declared terms: speed class (20, 50 or 100 output tokens per second), arrival rate (1–1,000,000 milli-requests per second), a truthful region (1–256 UTF-8 bytes, no control characters), a capacity tier that the observed physical memory meets, a probe location, and samples per mode (100–1,000). It SHALL also collect the calibration token rates and minimum funding for the measurement deployment. The calibration rates bind only the calibration agreements: the share's published prices are set by Auto-price or manual pricing and need not equal them. The wizard SHALL also offer **Share without qualification**, which approves no budget and publishes the share free and without a serving profile; the dashboard SHALL then offer the same budget approval later.

The wizard SHALL compute the run's maximum budget exactly as follows and display it as exact wei. The inputs are:

- E: the number of advertised generation endpoints (`chat`, `completions`);
- N: the samples per mode;
- C: the maximum concurrency;
- A: the declared arrival rate in milli-requests per second;
- P = ceil((2,048 × input_wei_per_1k + 256 × output_wei_per_1k) / 1,000): the maximum reserved cost of one paid request at the calibration rates.

The budget is computed from them in these steps:

1. For each concurrency c from 1 to C, the paid gateway sweep sends S_c = c × ceil(max(2E × N, c) / c) requests across its 2E modes.
2. The run also sends W paid requests outside the sweeps: 2E paid warm-ups, plus one paid completion echo probe when `completions` is advertised.
3. The number of funded agreements is K = max(1, ceil((ceil(60 × A / 1,000) + 2C) / (118 − 2E))). This keeps every agreement below the limit of 120 attempts per account per minute.
4. Paid requests rotate across the agreements, with the warm-ups and the echo probe on the first. Agreement k is therefore reserved for R_k = Σ_c ceil(S_c / K) requests, plus W for the first agreement.
5. Its opening funding F_k is the smallest integer that is at least the calibration minimum funding and satisfies F_k − floor(F_k × 50 / 10,000) ≥ R_k × P.
6. The budget is the sum of the F_k, shown with its platform fees, plus the estimated gas of K funding transactions. The gas is labeled as an estimate and kept outside the exact total.

Private-engine samples and warm-ups are unpaid and are not part of the budget.

The wizard SHALL state that:

- the funding is paid to this machine's provider wallet but is a non-refundable prepayment to the calibration deployment;
- each funding creates a platform-fee obligation for the provider;
- while the calibration deployment runs, it remits fee batches automatically from the provider wallet, spending its gas, when they reach 10^15 wei or the oldest obligation is 86,400 seconds old;
- gas is spent from both wallets;
- failed, stopped or aborted runs can spend budget.

It SHALL show the funding risk disclosure required before any opening or top-up. The desktop SHALL NOT spend until the user actively approves that exact budget. An approval SHALL authorize exactly one run for the share and settings it was given for, started automatically or by *Qualify now*, and SHALL say so. It SHALL lapse without spending when that share stops before the run starts, when an earlier wizard choice changes, or when the user cancels the scheduled run; it is used up when the run ends. Every later run SHALL need a new approval.

Total funding across the run SHALL NOT exceed the approved budget. The desktop SHALL NOT top up automatically. When the run would need more, it SHALL stop and require a newly approved budget. The paying wallet SHALL differ from the provider wallet; it MAY be the loaded consumer wallet when that wallet is not the provider wallet. Qualification funding SHALL follow the exact funding order and its recovery: journaled funding is offered as *Resume*, and no second payment is prepared while one is unresolved.

Qualification funding and paid requests SHALL use a native consumer context created only for the calibration deployment. That context SHALL have no catalog and no DHT discovery. It SHALL use its loopback development allowance only for the calibration deployment's literal-loopback endpoint. It SHALL keep the paying wallet's journal under that wallet's recorded `data_dir` base. When the paying wallet is the loaded consumer wallet, the context SHALL use that profile's base and account state under the same native coordination as the loaded consumer. Any other paying wallet SHALL use a base dedicated to qualification payments. The loaded consumer profile's `development` setting and origin validation SHALL NOT change. Calibration accounts SHALL be labeled as calibration wherever accounts are listed.

The calibration deployment SHALL carry no serving profile and SHALL bind every listener to literal loopback. Its DHT listener SHALL use loopback with no bootstrap peers and no libp2p serving, and it SHALL NOT publish its offers on the DHT. Because it runs while the share's published deployment is paused but still running, every calibration listener SHALL use a port distinct from every listener of the published deployment.

#### Scenario: No spending before budget approval

- **WHEN** the user opens the qualification step
- **THEN** the computed maximum budget and the funding risk disclosure are shown
- **AND** no quote is funded and no paid request is sent until the user approves that exact budget

#### Scenario: Budget from fixed inputs

- **GIVEN** a chat-only model (one generation endpoint, no `completions`), 100 samples per mode, maximum concurrency 2, a declared arrival rate of 2,000 milli-requests per second, calibration rates of `1000` wei input and `2000` wei output per 1,000 tokens, and a calibration minimum funding of `1000` wei
- **WHEN** the wizard computes the budget
- **THEN** it shows two agreements: the first funded with `519718` wei for 202 paid requests and the second with `514572` wei for 200 paid requests
- **AND** the exact total is `1034290` wei including `5170` wei of platform fees, with the gas of two funding transactions shown separately as an estimate

#### Scenario: The approved budget is a hard cap

- **GIVEN** an approved budget that the run's openings have fully used
- **WHEN** the run would need another opening or top-up
- **THEN** the run stops without funding and asks for a new budget approval

#### Scenario: Paying wallet equal to the provider wallet

- **WHEN** the selected paying wallet is the provider wallet
- **THEN** the wizard refuses it and asks for a distinct paying wallet

#### Scenario: Interrupted qualification funding

- **GIVEN** a qualification opening was journaled but its outcome is unknown
- **WHEN** the user returns to the qualification step
- **THEN** the pending funding is shown with *Resume*
- **AND** no new payment is prepared until it is resolved

#### Scenario: The calibration deployment stays on this machine

- **WHEN** qualification runs while the share's published deployment is paused
- **THEN** every calibration listener, its DHT listener included, is bound to literal loopback on a port that no listener of the published deployment uses, with no bootstrap peers and no libp2p serving
- **AND** none of the calibration deployment's offers is published on the DHT

#### Scenario: Origin validation is unchanged during qualification

- **GIVEN** a loaded consumer profile with development off and a qualification run in progress
- **WHEN** a network action of that profile targets a literal-loopback HTTP origin
- **THEN** the action is rejected under the profile's unchanged origin rules
- **AND** the profile's development setting is still off, and the run's accounts are labeled as calibration

### Requirement: Qualification evidence is journaled and never fabricated

The in-app qualification SHALL produce `chiral-performance-evidence-v2` evidence and a qualified ModelSpec that meet every requirement of the provider qualification procedure and are accepted by the native revalidation used at deployment and readiness. It SHALL build the serving profile from the managed engine's authenticated runtime observation, never from labels the user types. It SHALL sweep every concurrency from one through the advertised maximum on both the private engine and the paid gateway, covering every advertised generation endpoint, streaming and non-streaming on the gateway. It SHALL take at least 100 samples per mode, with prompts in the 128, 512, 1,024 and 2,048-token buckets (at least 20 per bucket) and exactly 256 output tokens per passing sample. A sample passes only when time to first token is at most 2,000,000 microseconds and decode rate meets the declared class. The run passes only with at least 95% joint attainment in each sweep, mode and bucket and overall, offered arrival within ±5% of the declared rate, and physical memory, sampled every 0.2 seconds, that stays within the configured reserve. Unsupported memory counters SHALL fail closed before paid dispatch. Runtime identity SHALL be checked before and after the run.

The run SHALL exclusively create its request journal before inspecting or dispatching anything. It SHALL durably record each prepared paid request key before dispatch and each completion mapping afterwards, and SHALL set both `X-Request-Id` and `Idempotency-Key`. The journal SHALL contain no prompts, output or credentials. After an interruption, the desktop SHALL keep the journal, ledgers, keys and account mapping, and SHALL reconcile every prepared-but-not-completed entry through the owning account's request status before allowing another attempt. It SHALL NOT replay a paid request.

A fully measured run that fails its declared class SHALL keep its evidence, SHALL produce no qualified ModelSpec, and that class SHALL NOT be published from it. Another attempt SHALL use a new output identity, a new credential and a separately approved budget. The desktop SHALL NOT relabel, rename, synthesize or delete evidence or journals. The calibration deployment's identity, ledger, accounts, funding journals and fee obligations SHALL be preserved under the protected data directory and listed with the share's liabilities. While the calibration deployment runs, it remits fee batches automatically at the aggregation thresholds. Forcing remittance of calibration fee obligations (`fees/remit`) SHALL be an explicit action that runs the calibration deployment on loopback only, without announcing it. The calibration deployment SHALL run only while the share's published deployment is in its qualification pause with its engine stopped, and SHALL NOT run while an engine of the published deployment is loaded, so the model is never loaded twice.

#### Scenario: A failed class cannot be published

- **GIVEN** a completed run in which one prompt bucket attained less than 95% joint success at the declared class
- **WHEN** the run ends
- **THEN** its evidence is retained, no qualified ModelSpec is produced and publishing that class is unavailable
- **AND** a new attempt requires a new budget approval

#### Scenario: Interrupted run is reconciled before retrying

- **GIVEN** the desktop exited while journal entries were prepared but not completed
- **WHEN** the user returns to qualification
- **THEN** each ambiguous entry is reconciled through request status before a new attempt is allowed
- **AND** no journaled request is dispatched again

#### Scenario: Evidence that fails native revalidation

- **WHEN** the produced evidence or profile does not pass the native deployment revalidation
- **THEN** the share is not published and the failure is shown

### Requirement: A qualification run can be stopped explicitly

While a qualification run is scheduled but not started, the Share page SHALL offer *Cancel scheduled qualification*, which lapses the budget approval without spending. While a run is in progress, the Share page SHALL offer *Stop qualification*. After the user confirms it, the desktop SHALL:

1. stop dispatching new requests;
2. wait for in-flight paid requests to complete or reach their request deadline, showing their count, unless the user chooses *Stop now* after a statement that interrupted requests settle under the known-usage or unknown-usage policy;
3. record every completion mapping it observes, and keep the journal, ledgers, keys and account mapping for reconciliation;
4. stop the calibration daemon through its graceful shutdown;
5. unless the share itself is being stopped, end the published deployment's qualification pause as *Qualification runs automatically in the share's first quiet hour* requires for a run that did not pass, so the share serves free again.

A stopped run SHALL produce no qualified ModelSpec, and its output identity SHALL NOT be reused. The desktop SHALL report the run as stopped only after the calibration daemon has exited. If the daemon does not exit, the desktop SHALL report the failure and keep showing the run as in progress.

#### Scenario: Stopping a run keeps its journal

- **GIVEN** a qualification run with paid requests in flight
- **WHEN** the user confirms *Stop qualification*
- **THEN** no new request is dispatched, the in-flight requests complete or reach their deadline, and the calibration daemon then exits
- **AND** the journal and ledgers remain, no qualified ModelSpec is produced, a new attempt requires a new budget approval, and the published deployment launches its engine again and serves free once its readiness probes pass

#### Scenario: Logout while qualifying points to Stop qualification

- **GIVEN** a qualification run in progress
- **WHEN** the user chooses Log out
- **THEN** logout is refused with an explanation that the run must be stopped first with *Stop qualification*
- **AND** the run keeps running

### Requirement: Free and embedding shares publish after readiness without speed qualification

A share is *free* while its input price, output price and minimum funding are all exactly zero. A free generation share is a free share whose model advertises `chat` or `completions`. A new generation share SHALL be first published free and without a serving profile (*New shares start free in a launch window*), whether or not a qualification budget was approved. That first publication SHALL:

- require no paying wallet, qualification budget, funding, calibration deployment or qualification run, and spend nothing;
- fix the share's input and output token rates and its minimum funding at exactly `"0"`, which the publish terms show with the reason, and carry the free allowance of *Free offers carry a signed free allowance*;
- configure the share's model with no serving profile (`serving: null`), under the free-generation exception of `inference-commercial-terms` (*Native deployment constraints cannot be bypassed by declaration validity*).

A free share SHALL be published only after the daemon's readiness probes pass on the published deployment, started without publishing: exact tokenizer and chat-template identity, the capability probe of every advertised endpoint, and a real inference whose usage the engine reports. When a probe fails, the share SHALL NOT be published and the wizard SHALL show the daemon's reason. A share whose model has no serving profile SHALL carry no speed class, capacity tier, qualification identity or evidence, and no performance remedy or waiver. Wherever the desktop shows it, on the Share page, the dashboard, the publish terms and the consumer surfaces of *Unqualified generation offers are labeled for consumers*, such a generation share SHALL be labeled **Unqualified — no speed guarantee**, and it SHALL NOT be presented as qualified. After a passing run is promoted into its deployment (*A passing run is promoted into the published deployment in place*), a free share carries its serving profile and speed class and is not labeled **Unqualified**.

Without a valid qualification (*Paid generation shares require in-app qualification within an approved budget*), the desktop SHALL NOT publish any nonzero input price, output price or minimum funding for a generation share, whether Auto-price or the user would set it, and SHALL explain that a price requires qualification first. A share's prices SHALL change only by new signed offer revisions (*A running share is repriced by a new signed offer revision*), never by restarting it with different terms, and accounts keep the terms they accepted.

An embedding share SHALL have zero output price and zero output limit, SHALL carry no generation serving profile or speed class, SHALL be published only after the daemon's readiness probes pass, and SHALL state that no generation-speed guarantee applies; it SHALL NOT require the generation speed qualification. Its first publication is free, like every new share's. Because the book's *ready to charge* requires "a valid qualification", which an embedding share cannot hold, an embedding share is never ready to charge: Auto-price SHALL be unavailable for it, with that reason shown (*Auto-price sets the price from launch reputation*), and it has no launch window. Its input price MAY be made nonzero only by manual pricing (*Manual pricing applies when Auto-price is off*), which needs no qualification for an embedding share, and its output price SHALL stay zero. An embedding share whose input price and minimum funding are zero is free under the opening rules but is not a free generation share, and keeps the embedding statement rather than the **Unqualified** label.

#### Scenario: A free share skips qualification

- **GIVEN** a generation model and no qualification run for it
- **WHEN** the user chooses *Share without qualification* in the qualification step
- **THEN** no paying wallet or budget is requested, no calibration daemon starts and nothing is funded
- **AND** the share is published free and unqualified, and the dashboard states that no nonzero price can be published until a qualification passes

#### Scenario: A free share publishes after readiness without a serving profile

- **GIVEN** a new generation share whose final reachability check has passed
- **WHEN** the daemon's readiness probes pass and the user publishes
- **THEN** the published offer carries `serving: null`, token rates of `"0"`, a `min_funding_wei` of `"0"` and the share's free allowance
- **AND** the Share page labels the share **Unqualified — no speed guarantee** and shows no speed class

#### Scenario: Readiness failure blocks a free share

- **GIVEN** a free share whose model fails the real-inference readiness probe
- **WHEN** the user tries to publish
- **THEN** no offer is published and the daemon's reason is shown

#### Scenario: A price requires qualification first

- **GIVEN** a running generation share in manual pricing whose model has no serving profile
- **WHEN** the user enters a nonzero output price or a minimum funding of `0.01` CFX on the dashboard
- **THEN** the value is not published, and the dashboard explains that a priced share needs a valid qualification first
- **AND** it offers the qualification budget approval, or *Qualify now* when an approved run is pending

#### Scenario: Embedding share without a speed class

- **GIVEN** a running embedding share, first published free
- **WHEN** the user sets, in manual pricing, a nonzero input price and a zero output price
- **THEN** the desktop publishes the new price as a new offer revision without requesting a speed qualification
- **AND** the offer carries no speed class and a zero output price

### Requirement: New shares start free in a launch window

Every new share SHALL be first published free (book *Author decisions* item 8): its input price, output price and minimum funding exactly `"0"`, with the free allowance of *Free offers carry a signed free allowance*. A new generation share is first published without a serving profile, as *Free and embedding shares publish after readiness without speed qualification* requires. This holds whatever target or manual price the pricing step records. A new share SHALL NOT inherit nonzero prices, a minimum funding, calibration rates, a qualification, a budget approval, an Auto-price target, floor or step, or a review history from an earlier share as a preselected choice. This includes an earlier paid share of the same installation or wallet.

With Auto-price on, a new share's first free period is its **launch window** (book *Share online* step 4). It SHALL last at least 24 hours from the share's first publication, measured by the desktop, and SHALL end only at an Auto-price review whose up rule holds (*Auto-price sets the price from launch reputation*). That rule requires the book's *ready to charge*: the catalog's launch criteria met, together with a valid qualification of the share, which the desktop checks itself; during the launch window it also requires that 24 hours have passed since the first publication. During the launch window the share SHALL stay free whatever floor the user chose. While the window lasts, the Share page SHALL show that the share is launching, how long it has been published, each launch criterion, met or unmet, as the catalog reports it, and whether the share has a valid qualification. With Auto-price off, and for an embedding share, for which Auto-price is unavailable, the share has no launch window: it stays free until the user sets a manual price (*Manual pricing applies when Auto-price is off*).

*Share again* on a stopped share (*Sharing is an explicit, ordered wizard*) is not a new share. It SHALL offer the stopped share's own accepted terms unchanged, so that accounts opened under them can be served again: its last published prices and minimum funding, its free allowance, its serving profile and its pricing mode, with Auto-price's target, floor and current step. A stopped share whose last published price is nonzero SHALL be published again only while it still has a valid qualification. Every check of *Publishing requires an accepted share disclosure* SHALL apply again, including a new disclosure acceptance. A share that had left its launch window SHALL NOT re-enter it; a share still in its launch window continues it, counted from its first publication. Choosing a different installation, serving setting, provider wallet or provider profile SHALL make a new share, which starts free.

#### Scenario: A new share starts free

- **GIVEN** a generation model that this machine could qualify for paid generation, and an Auto-price target of `1000` wei input and `2000` wei output per 1,000 tokens
- **WHEN** the user publishes a new share of it
- **THEN** its offers carry token rates and a minimum funding of `"0"`, `serving: null` and the share's free allowance
- **AND** the Share page shows the share in its launch window

#### Scenario: The launch window lasts at least 24 hours

- **GIVEN** a new share with Auto-price on, first published 18 hours ago with a passing qualification already promoted into its deployment, from a wallet whose earlier shares give it a catalog record that reports its launch criteria met
- **WHEN** the 18-hour review runs
- **THEN** the share stays free and in its launch window
- **AND** only a review at least 24 hours after its first publication whose up rule holds publishes its first nonzero step

#### Scenario: Earlier prices are not carried forward

- **GIVEN** a stopped paid share of an installation whose accounts hold available balance
- **WHEN** the user starts a new share of the same installation from the wizard
- **THEN** the new share starts free, with zero prices and minimum funding, and no Auto-price target or floor preselected from the stopped share
- **AND** before publishing, the wizard lists the paid share's accounts as accounts the free share will not serve, with their exact total liability, and requires acknowledgement

#### Scenario: Sharing a stopped paid share again keeps its terms

- **GIVEN** a stopped share at the Auto-price 50% step, whose accounts hold available balance and whose qualification native revalidation still accepts
- **WHEN** the user chooses *Share again* on it
- **THEN** the wizard offers its last published prices, serving profile, Auto-price target, floor and 50% step unchanged instead of starting free
- **AND** it publishes only after every publish check passes and the disclosure is accepted again, after which those accounts are served again and the next review comes 6 hours later

### Requirement: Free offers carry a signed free allowance

Every free offer a share publishes SHALL carry the per-account free allowance of book *Author decisions* item 10. The schema v2 offer declares it in `free_allowance`, and `inference-commercial-terms`, as `specify-p2p-provider-transport` changes it, has the provider serve a free account only within it (*Free accounts are served only within their signed allowance*). It has two members: `tokens`, the most input and output tokens together that one account may use, and `lifetime_secs`, the number of seconds after the account's opening at which the provider stops serving it. The pricing step SHALL preset 20,000 tokens and 24 hours (86,400 seconds). It SHALL let the user change either within the bounds that change defines, and SHALL refuse a value outside them, showing the bounds, rather than clamp it. The desktop SHALL write the allowance into the generated configuration, and SHALL show it in the publish terms, the disclosure and the dashboard. A priced offer carries no allowance. A changed allowance on a running share SHALL be published as a new offer revision through *A running share is repriced by a new signed offer revision*; accounts already opened keep the allowance they were opened under. The Share page SHALL state that an account's lifetime keeps running while the share is paused, drained or stopped, and that a consumer whose allowance has ended needs a new account for further service.

#### Scenario: The allowance defaults

- **WHEN** the user reaches the pricing step of a new share
- **THEN** the free allowance shows 20,000 tokens and 24 hours
- **AND** the share's free offers carry `free_allowance` `{"lifetime_secs":86400,"tokens":20000}`

#### Scenario: An allowance outside the bounds is refused

- **WHEN** the user enters a token count below the lower bound that `specify-p2p-provider-transport` defines
- **THEN** the pricing step refuses it and shows the bounds
- **AND** it does not raise the value to the bound on the user's behalf

#### Scenario: A changed allowance applies to new accounts only

- **GIVEN** a running free share with accounts opened under a 20,000-token allowance
- **WHEN** the user changes the allowance to 50,000 tokens and confirms
- **THEN** a new offer revision carries an allowance of 50,000 tokens
- **AND** the existing accounts keep their 20,000-token allowance

### Requirement: Qualification runs automatically in the share's first quiet hour

When a qualification budget is approved for a generation share (*Paid generation shares require in-app qualification within an approved budget*), the desktop SHALL run that qualification automatically while the share is published, without a further prompt (book *Share online* step 4). It SHALL choose the start as follows:

- **Warm-up.** No automatic run SHALL start within one hour of the share's first publication.
- **Quiet.** After the warm-up, the run SHALL start at the first moment at which no request, from the network or local, has been admitted to the share for the previous 15 minutes and none is in flight. That moment begins the share's *first quiet hour*.
- **Fallback.** If no such moment has come 12 hours after the share's first publication, the run SHALL start then.
- **Qualify now.** While an approved run is pending, the dashboard SHALL offer *Qualify now*, which starts the run at once after a confirmation.
- **Deferral.** A run SHALL NOT start while the share is draining, not ready, failing its reachability check or being repriced, or while the paying wallet's verified balance is below the approved budget plus its estimated gas. The dashboard SHALL show the reason, and the run SHALL start at the first moment the rules above allow after the reason clears.

These times SHALL be measured on a monotonic clock while the desktop runs. A budget approval lapses when the share stops, so a share published again starts no run without a new approval.

The desktop SHALL offer a budget approval, schedule a run or offer *Qualify now* only for a generation share whose published model carries no serving profile. A share whose model carries one, including a model withdrawn by a performance-withdrawal latch, SHALL NOT be qualified again in place: its accounts may hold prepaid balances under that profile, and a failed run would otherwise leave the share without the profile they were opened under. The dashboard SHALL state this.

The run SHALL proceed in this order:

1. **Pause.** The published deployment enters a *qualification pause*. It SHALL withdraw its published offers as a drain does, and SHALL answer every new inference request and every new opening with a retryable busy response, HTTP 503 with the error code `qualification_in_progress` and a `Retry-After` header, which admits nothing and charges nothing. The `Retry-After` value SHALL be an integer number of seconds from 1 to 3,600: the estimated remaining time of the pause, rounded up and clamped to that range, so that the local API server of `specify-desktop-local-api-server`, which passes the header on only within that range, and native consumers, which treat this response as retryable and not admitted under `specify-p2p-provider-transport`, receive a usable value. The response SHALL NOT be a payment, allowance or funding error, and nothing in it SHALL invite a consumer to retry as a new paid request. It SHALL let in-flight requests finish within their existing deadlines, SHALL report not ready with that reason, and SHALL then stop its managed engine, releasing the shared devices. It SHALL keep running, with its identity, libp2p reachability, relay reservations, ledger, maintenance and automatic fee remittance, and SHALL keep serving request status, receipts, cancellation and account inspection, and verifying and crediting funding it acknowledged before the pause, exactly as when it is not paused. Local use of the installation is refused (*Local use of a shared model passes through admission*).
2. **Measure.** The calibration deployment then runs the qualification of *Paid generation shares require in-app qualification within an approved budget* and *Qualification evidence is journaled and never fabricated*, within the approved budget and unchanged in every other respect.
3. **End.** The calibration daemon stops through its graceful shutdown. A passing run is then promoted as *A passing run is promoted into the published deployment in place* requires. After a failed run, a run stopped with *Stop qualification*, or a run whose calibration daemon could not start, the published deployment SHALL end its pause: it SHALL launch its managed engine again with its unchanged configuration, and SHALL admit requests and publish offers again only after its readiness probes pass. The share SHALL stay free and without a serving profile, and the dashboard SHALL report the outcome and offer another run with a newly approved budget.

The pause counts against the share's availability: the catalog finds the provider without a current offer, and this change claims no maintenance exemption from the launch reputation. The qualification step and the dashboard SHALL show an estimated run duration, labeled as an estimate and derived from the run's request counts and declared speed class. They SHALL state that the ready-to-charge threshold of 95% availability allows at most 5% of the observed time to be unavailable, which is 72 minutes of the first 24 hours, so that a pause of D minutes in that period leaves at most 72 − D minutes for any other unavailability; beyond that, the launch window lasts until the 14-day availability recovers to 95%.

Stopping and launching the published deployment's engine as this requirement orders are planned steps of an authorized run, not an automatic restart after a failure. If either daemon fails during the run, the desktop SHALL stop the run as *A qualification run can be stopped explicitly* requires, SHALL report the failure, and SHALL NOT restart a failed daemon automatically.

#### Scenario: The run waits for a quiet moment after the warm-up

- **GIVEN** a share first published at 09:00 with an approved budget, which admitted requests until 10:40 and none after
- **WHEN** 10:55 arrives with no request admitted for 15 minutes and none in flight
- **THEN** the run starts at 10:55
- **AND** with no traffic at all, no run would have started before 10:00

#### Scenario: The fallback starts a run on a busy share

- **GIVEN** a share with an approved budget that has admitted a request in every 15-minute period since it was first published
- **WHEN** 12 hours have passed since its first publication
- **THEN** the run starts: the share pauses, and its in-flight requests finish before its engine stops

#### Scenario: The pause refuses consumers retryably

- **GIVEN** a share in its qualification pause
- **WHEN** the owner of a free account sends a request
- **THEN** it receives HTTP 503 `qualification_in_progress` with a `Retry-After` header whose value is an integer from 1 to 3,600 seconds, nothing is dispatched and its allowance is unchanged
- **AND** the published deployment publishes no offer, reports not ready, and stopped its engine before the calibration daemon started

#### Scenario: Recovery routes stay available during the pause

- **GIVEN** a share in its qualification pause and a consumer whose earlier request's outcome is unknown
- **WHEN** the consumer asks for that request's status and receipt
- **THEN** the paused deployment answers them as it would when not paused
- **AND** only new inference requests and new openings receive HTTP 503 `qualification_in_progress`

#### Scenario: A qualified share is not qualified again in place

- **GIVEN** a share whose published model carries a serving profile and whose performance-withdrawal latch has withdrawn it
- **WHEN** the user opens the dashboard
- **THEN** it offers no budget approval, *Qualify now* or scheduled run, and states that a model with a serving profile is not qualified again in place

#### Scenario: A failed run leaves the share free

- **GIVEN** a run that fails its declared class
- **WHEN** the run ends
- **THEN** the published deployment launches its engine again with its unchanged configuration and serves free once readiness passes
- **AND** its offers still carry `serving: null`, and another run needs a new budget approval

#### Scenario: The pause is shown as lost availability

- **WHEN** the user approves a budget whose run is estimated at 60 minutes
- **THEN** the qualification step states that the pause counts as unavailable, and that it leaves at most 12 minutes of other unavailability in the first 24 hours before availability falls below 95%

### Requirement: A passing run is promoted into the published deployment in place

After a passing run, the desktop SHALL promote its qualified ModelSpec, with its serving profile and evidence file, into the share's existing published deployment. It SHALL keep the deployment's provider wallet, persisted DHT identity and peer ID, `data_dir` base, ledger, acknowledgements, journals and offer-revision counter, and SHALL NOT create a new deployment, identity or ledger for the result. It SHALL:

1. stop the published deployment's daemon through its graceful shutdown while it is still paused;
2. regenerate its configuration with the qualified ModelSpec and nothing else changed: the same installation, serving settings, prices, minimum funding and free allowance;
3. start it without publishing, and require native deployment revalidation of the evidence and the daemon's readiness probes to pass;
4. run the final reachability check of *Reachability is verified through a relay before publishing* again;
5. resume publication, and confirm it from the daemon's published-offer list.

These steps need no prompt beyond the disclosure accepted when the share was published, which describes them. From then on the deployment's model SHALL keep its serving profile at every price that Auto-price or manual pricing selects, free included, so that a price change never changes execution terms. Its free offers then carry the serving profile and speed class together with the free allowance, and accounts opened under them stay servable at every later price, because the daemon's execution-compatibility rule for agreed models compares no prices. A free agreement under a serving profile follows the same performance rules as any agreement under that profile: an eligible miss settles with a waiver of a cost that is zero, and it counts toward the durable performance-withdrawal latch. The dashboard SHALL state this.

Free accounts opened under the share's offers without a serving profile before the promotion SHALL keep being served after it, under their own signed free allowance, with the same model and settings and no speed promise (book *Share online* step 4). This rests on the pre-promotion free-agreement exception of `inference-commercial-terms` (*Existing agreements permit only evidence-only renewal of identical terms*, as this change modifies it): the promotion adds only the serving profile, so every other execution setting stays identical. Those accounts SHALL NOT acquire the serving profile, speed class, performance remedy or waiver; their requests SHALL NOT be performance-accounted and SHALL NOT count toward the performance-withdrawal latch. They end, as before the promotion, when their allowance is used or their lifetime passes. The publish disclosure SHALL say so in advance, and after the promotion the dashboard SHALL show how many such accounts remain open with allowance left, labeled as served without a speed promise.

If revalidation, readiness or the reachability check fails, the desktop SHALL NOT publish, SHALL keep the deployment drained and SHALL report the failure. It SHALL offer two explicit actions: retry the promotion, or restore the configuration without the serving profile so that the share serves free and unqualified again. It SHALL take neither automatically, and SHALL NOT automatically restart a daemon that fails during these steps.

#### Scenario: A passing run keeps the deployment's identity

- **GIVEN** a share in its launch window, last published at offer revision 7 under peer ID `P`, whose run passes at speed class 20
- **WHEN** the run is promoted
- **THEN** the daemon restarts with the same provider wallet, peer ID `P`, ledger and revision counter, and publishes a revision above 7
- **AND** that offer is free, carries the free allowance, and carries the qualified serving profile with speed class 20 instead of the **Unqualified** label

#### Scenario: Free accounts opened before the promotion keep being served

- **GIVEN** a free account opened under the share's offer with `serving: null`, with allowance remaining and its lifetime not passed
- **WHEN** the promotion completes and the account's owner sends a request whose worst case fits the remaining allowance
- **THEN** the promoted deployment admits it and settles it at zero cost against the account's allowance, with no speed promise, performance accounting, waiver or withdrawal-latch count
- **AND** the dashboard counts the account among the pre-promotion free accounts still served, and a request after its allowance is used or its lifetime passes is refused as before the promotion

#### Scenario: A failed promotion is not published

- **GIVEN** a passing run whose evidence native revalidation rejects when the published deployment restarts
- **WHEN** the promotion fails
- **THEN** no offer is published and the failure is shown
- **AND** the user can retry the promotion or restore the unqualified free configuration, and neither happens automatically

### Requirement: Auto-price sets the price from launch reputation

The pricing step SHALL offer an **Auto-price** checkbox, selected by default (book *Author decisions* items 8 and 11). It SHALL be unavailable, with the reason, when the provider profile names no catalog; such a share uses manual pricing. It SHALL also be unavailable for an embedding share, with the reason that an embedding share cannot hold a valid qualification and so can never be ready to charge; such a share uses manual pricing (*Free and embedding shares publish after readiness without speed qualification*). With Auto-price selected, the user SHALL set:

- a **target**: input and output token rates and a minimum funding, validated as a paid price under *Pricing uses exact amounts and shows the platform fee*;
- a **floor**: one of the steps below, preset to 0% (free).

**Steps.** Auto-price SHALL keep the share's price at one of five steps, s ∈ {0, 25, 50, 75, 100} percent of the target. At step s each token rate is floor(target rate × s / 100) wei, computed with exact integer arithmetic. The minimum funding is `"0"` at step 0 and the target's minimum funding at every other step. Step 0 is free and carries the free allowance. The pricing step SHALL show the five step prices exactly, and SHALL refuse a target whose 25% step would have both token rates zero.

**Reviews.** Auto-price SHALL review the share every 6 hours, the first review 6 hours after the share's first publication or after *Share again* publishes it. It SHALL NOT review a share sooner than 6 hours after the previous review, including across a restart of the desktop or the daemon, and SHALL record each review's time durably with the share. A review that falls due during a qualification run, a promotion, a drain or a repricing, or while the daemon is not ready, SHALL run once that ends. A review SHALL move the price at most one step:

- **Down**, when any of these is established: the service score is below 80; the 14-day availability is below 95%; or the payment-gated score is below 45 with at least 5 eligible votes in the last 14 days. The price moves one step down, but never below the floor. During the launch window the price stays free.
- **Up**, when every down condition is established as not holding and all of these hold: the provider is ready to charge, meaning that the catalog reports its launch criteria met (while the share is free, currently; while it charges, at the end of the provider's latest free period) and that the share has a valid qualification, which the desktop checks itself because the catalog does not assess qualification; the service score is at least 90; and, during the launch window, 24 hours have passed since the share's first publication. The price moves one step up, and from step 0 to the larger of 25% and the floor.
- **Hold**, otherwise. A down condition that is established takes precedence over the up rule.

**Inputs.** At each review the desktop SHALL read, natively and only from the provider profile's `catalog_url`, for this provider's wallet and peer ID:

- the launch reputation that `specify-provider-launch-reputation` defines: the service score, the 14-day availability, the launch criteria with each unmet criterion, whether the provider is free now, whether the launch criteria were met at the end of its latest free period, the observer status and the time the figures were computed;
- the payment-gated reputation: the score and its eligible vote counts, in total and in the last 14 days.

An input is *unavailable* when the catalog cannot be reached or answers with an error, reports its observer degraded, reports no record for this provider or omits the input, or reports figures computed more than 6 hours before the review. Auto-price SHALL NOT replace an unavailable input with an estimate, a default, its own observation of the provider or a figure from another catalog. A down condition SHALL be established only from available inputs. The payment-gated condition is established as not holding when the score is at least 45, when the total eligible vote count is below 5, or when the catalog reports fewer than 5 eligible votes in the last 14 days. An up move SHALL need every up input available and every down condition established as not holding. A review that can establish neither SHALL hold, and SHALL record which input was unavailable.

**Publication.** Each change SHALL be published as a new signed offer revision through *A running share is repriced by a new signed offer revision*. Existing accounts keep their agreed terms, and signed quotes keep theirs until they expire. A nonzero step SHALL be published only while the share has a valid qualification. Auto-price SHALL NOT review or change the price of an embedding share. Auto-price SHALL NOT approve a budget, start a qualification run or spend anything.

**Changes.** The dashboard SHALL let the user change the target and the floor, after a confirmation. A change SHALL take effect at the next review. That review first adjusts the current step: it recomputes the step's prices from the new target and, outside the launch window, raises the step to the floor when it is below it. Unchecking Auto-price SHALL keep the current price and switch the share to manual pricing. Checking it again SHALL require a target and a floor; the next review, no sooner than 6 hours after the previous one, first sets the current step to the highest step whose token rates do not exceed the current ones, raised to the floor outside the launch window. Such an adjustment is not a step move: the review then moves the price at most one step under the rules above, and SHALL publish at most one new offer revision in all.

**Dashboard.** The Share dashboard SHALL show whether Auto-price is on; the target, the floor and the five step prices in exact amounts; the current step; whether the share is in its launch window; the time of the next review; and the last review's decision and reason, naming the rule that applied and each input as the catalog reported it with the catalog origin and computation time, or naming the input that was unavailable. It SHALL keep every review from the last 30 days and at least the last 100 reviews, and SHALL discard any review that is both older than 30 days and outside the last 100; because reviews are at least 6 hours apart, the history holds at most 121 reviews. Each entry SHALL give the review's time, decision and reason, and for a change its steps and offer revision. The history SHALL be kept natively in the share's protected local state, SHALL contain no secrets, and SHALL be shown as plain text. The dashboard SHALL state that Auto-price follows one catalog's observation, that other catalogs can disagree, and that the figures are a signal that consumers also see, not a catalog's decision about who may charge.

#### Scenario: Auto-price is selected by default

- **GIVEN** a new generation share whose provider profile names a catalog that reports the profile's network identity
- **WHEN** the user opens the pricing step
- **THEN** Auto-price is selected, the floor is 0% (free), and the target's five step prices are shown once a target is entered

#### Scenario: Leaving the launch window

- **GIVEN** a share in its launch window, first published 24 hours ago, with a passing qualification promoted into its deployment, and a catalog that reports its launch criteria met, a service score of 96, a 14-day availability of 97% and no payment-gated votes
- **WHEN** the review runs
- **THEN** the desktop publishes the 25% step as a new offer revision, with a `null` free allowance and the target's minimum funding
- **AND** accounts opened under the free revisions keep their free terms and allowances

#### Scenario: Step prices are exact

- **GIVEN** a target of `1001` wei input and `2002` wei output per 1,000 tokens
- **WHEN** the pricing step shows the steps
- **THEN** the 25%, 50%, 75% and 100% steps are `250`/`500`, `500`/`1001`, `750`/`1501` and `1001`/`2002` wei, and the 0% step is free

#### Scenario: A down condition takes precedence

- **GIVEN** a share outside its launch window at the 75% step, with a valid qualification, whose catalog reports a service score of 92, a 14-day availability of 94% and launch criteria met at the end of its latest free period
- **WHEN** the review runs
- **THEN** the price moves to the 50% step

#### Scenario: Paid votes lower the price

- **GIVEN** a share at the 50% step whose catalog reports a service score of 95, a 14-day availability of 99%, and a payment-gated score of 40 with 6 eligible votes in the last 14 days
- **WHEN** the review runs
- **THEN** the price moves to the 25% step

#### Scenario: The floor bounds a step down

- **GIVEN** a share outside its launch window with a floor of 50%, at the 50% step, whose catalog reports a service score of 70
- **WHEN** the review runs
- **THEN** the price stays at the 50% step

#### Scenario: An unavailable catalog holds the price

- **GIVEN** a share at the 75% step whose catalog cannot be reached
- **WHEN** the review runs
- **THEN** the price is unchanged, no offer revision is published, and the dashboard records that the review held because the catalog was unavailable, showing no figure for it

#### Scenario: A step down to free

- **GIVEN** a share with a floor of 0% at the 25% step, whose catalog reports a service score of 75
- **WHEN** the review runs
- **THEN** the desktop publishes a free revision with a minimum funding of `"0"`, the free allowance and the deployment's serving profile
- **AND** accounts opened at the 25% step keep their prices and balances

#### Scenario: An unqualified share is never priced

- **GIVEN** a share in its launch window whose qualification run failed, and a catalog whose figures would otherwise meet the up rule
- **WHEN** reviews run after 24 hours
- **THEN** the share stays free, and the dashboard states that a nonzero price needs a valid qualification

#### Scenario: Auto-price is unavailable for an embedding share

- **GIVEN** a new embedding share whose provider profile names a catalog that reports the profile's network identity, a service score of 95 and a 14-day availability of 98% for this provider
- **WHEN** the user opens the pricing step
- **THEN** the Auto-price checkbox is not selected and cannot be selected, and the step shows the reason that an embedding share cannot hold a valid qualification and so can never be ready to charge
- **AND** manual pricing remains available, accepting a nonzero input price with a zero output price without a qualification, and no Auto-price review is scheduled for the share

#### Scenario: Reviews stay 6 hours apart

- **GIVEN** a share whose last review ran at 10:00, and whose published daemon restarted at 11:00 for a promotion
- **WHEN** the promotion completes at 11:30
- **THEN** no review runs before 16:00
- **AND** after a desktop exit and *Share again* at 12:00, no review would run before 18:00

### Requirement: Manual pricing applies when Auto-price is off

With Auto-price off, the desktop SHALL change a share's price only when the user sets new prices on the Share dashboard and confirms a statement that shows the exact new terms and says that they apply to new accounts only. The values SHALL satisfy *Pricing uses exact amounts and shows the platform fee*. A nonzero price for a generation share SHALL be published only while the share has a valid qualification; otherwise the dashboard SHALL explain that a price needs a valid qualification first, and SHALL offer the budget approval or *Qualify now*. An embedding share's input price needs no qualification. Each change SHALL be published as a new offer revision through *A running share is repriced by a new signed offer revision*.

The launch reputation is a signal, not a gate: the desktop SHALL NOT refuse a manual price because of the catalog's figures. While the catalog does not report the provider's launch criteria met, the confirmation SHALL state that consumers see its launch reputation as incomplete. The dashboard SHALL keep showing the launch reputation in manual pricing.

#### Scenario: A manual price after a passing run

- **GIVEN** a share in manual pricing with a passing qualification promoted into its deployment, whose catalog reports 30 probes and so reports its launch criteria unmet
- **WHEN** the user sets `1000` wei input and `2000` wei output per 1,000 tokens with a minimum funding of `0.01` CFX and confirms
- **THEN** the confirmation states that consumers see the launch reputation as incomplete, and the desktop publishes those prices as a new offer revision
- **AND** accounts opened under the earlier free revisions keep their free terms

### Requirement: A running share is repriced by a new signed offer revision

The desktop SHALL change a running share's token rates, minimum funding or free allowance only through an operator-only action on the managed daemon's loopback administration listener, without draining and without restarting the daemon or its engine. The daemon SHALL validate the new values as its configuration validation would, and SHALL refuse invalid values without changing anything. In particular it SHALL refuse a nonzero token rate or minimum funding for a generation model without a serving profile; a free price with a nonzero minimum funding or without a free allowance; a priced price with a free allowance; and a nonzero output rate for an embedding model. On success it SHALL durably record the new values so that a restart keeps them, SHALL sign and publish at once a new offer revision with a higher revision number, and SHALL stop publishing offers with the previous values. It SHALL NOT change any other term, any accepted agreement or balance, any quote it already signed, or any credential. The desktop SHALL write the same values into the generated configuration, and SHALL report the change as published only after the daemon's published-offer list shows the new revision. It SHALL NOT reprice a share while the share is paused for qualification, draining, being promoted or not ready.

#### Scenario: Repricing keeps serving

- **GIVEN** a qualified share at the free step with a request in flight on a free account
- **WHEN** Auto-price publishes the 25% step
- **THEN** the engine keeps running, the in-flight request completes under the account's free terms, and a new offer revision with the 25% prices and a `null` free allowance is published
- **AND** a quote signed earlier at the free price keeps its terms until it expires

#### Scenario: An unqualified share cannot be repriced to a nonzero price

- **GIVEN** a running generation share whose model has no serving profile
- **WHEN** the repricing action is called with a nonzero output rate
- **THEN** the daemon refuses it, and its published offers and revision are unchanged

### Requirement: Unqualified generation offers are labeled for consumers

Wherever the native desktop shows a consumer a signed offer, quote or account whose model advertises `chat` or `completions` and carries no serving profile (`serving: null`), it SHALL show the label **Unqualified — no speed guarantee** in place of a serving profile. This covers at least the Marketplace offer card and its serving-profile area (book A.4), the provider and funding terms inspection, the acceptance shown before opening an account, the Playground's account selection, the account views of Usage and Account, and, once `desktop-local-chat` exists, network entries in the Chat model picker and network threads. The desktop SHALL derive the label only from the signed declaration, whatever the offer's prices, and never from a provider's name, description or other unsigned claim. For such a model it SHALL NOT show a speed class, capacity tier, admitted load, measured hardware or qualification provenance, and SHALL state that no performance remedy or waiver applies. A model without `chat` or `completions` and without a serving profile SHALL keep the statement that no generation-speed guarantee applies. The label SHALL be text, not only a color or icon. Machine-readable CLI output SHALL keep reporting the signed `serving: null` unchanged and SHALL NOT add a speed or qualification claim for such a model.

#### Scenario: A free share on the Marketplace

- **GIVEN** a consumer viewing a signed offer for a `chat` model with `serving: null` and token rates of `"0"`
- **WHEN** the Marketplace shows its offer card
- **THEN** the card reads **Unqualified — no speed guarantee** where a serving profile would be shown
- **AND** it shows no speed class, capacity tier or qualification provenance, and states that no waiver applies

#### Scenario: Embedding offer keeps its statement

- **GIVEN** a signed offer for an `embeddings`-only model with `serving: null`
- **WHEN** the Marketplace shows its offer card
- **THEN** the card states that no generation-speed guarantee applies and does not show the **Unqualified** label

### Requirement: Pricing uses exact amounts and shows the platform fee

The pricing step SHALL take input and output prices per 1,000 tokens and the minimum funding in CFX: as the Auto-price target (*Auto-price sets the price from launch reputation*), or, with Auto-price off, as the manual price the share may later publish (*Manual pricing applies when Auto-price is off*). The dashboard SHALL take later manual prices, targets and floors in the same way. Whatever is entered, a new share's first publication is free (*New shares start free in a launch window*), and a nonzero price SHALL be published only as those requirements allow. The desktop SHALL convert each amount exactly into a canonical decimal wei string under the native CFX conversion rules, and SHALL display amounts without floating point or rounding. A paid price SHALL have a positive minimum funding and at least one nonzero token rate. A price whose token rates are both zero SHALL have a minimum funding of exactly `"0"`; any other value SHALL be rejected. An embedding share SHALL have a zero output price. While a share is free, its token rates and minimum funding are `"0"` and shown read-only with the reason. The published prices are independent of the calibration rates of the share's qualification run.

The pricing step SHALL show that the platform fee is fixed at 50 basis points of each accepted funding payment (`fee = floor(funding_wei × 50 / 10000)`), owed by the provider to the platform wallet named by the provider profile. It SHALL show exact fee and net-credit amounts at the minimum funding of the target or manual price, and SHALL state that the fee is a funding charge, not a per-request charge; that the provider pays remittance gas; and that consumer gas is separate. For a free price it SHALL instead state that its openings carry no funding and so create no platform fee. The fee rate SHALL NOT be editable. Prices SHALL apply only to the offers this share publishes; changing them later SHALL produce new offers and SHALL NOT alter accepted agreements.

#### Scenario: The first publication is free whatever the target

- **GIVEN** a new generation share with an Auto-price target of `1000` wei input and `2000` wei output per 1,000 tokens and a minimum funding of `0.01` CFX
- **WHEN** the user reaches the publish step
- **THEN** the terms to be published show token rates and a minimum funding of `"0"` read-only, with the reason
- **AND** the pricing step shows the target and its five step prices as the prices Auto-price may publish later

#### Scenario: Exact conversion

- **GIVEN** an embedding share in manual pricing
- **WHEN** the user enters an input price of `0.000000000000000001` CFX
- **THEN** the share's input rate is the wei string `"1"`
- **AND** an entry with 19 fractional digits is rejected rather than rounded

#### Scenario: Fee shown at the minimum funding

- **WHEN** the minimum funding is `0.01` CFX
- **THEN** the wizard shows a minimum funding of `10000000000000000` wei, a platform fee of `50000000000000` wei and a net prepaid credit of `9950000000000000` wei

#### Scenario: Paid share without a positive minimum

- **WHEN** a token rate is nonzero and the minimum funding is zero
- **THEN** the pricing step rejects the values

#### Scenario: Free share requires zero minimum funding

- **GIVEN** an embedding share whose input and output prices are both zero
- **WHEN** the user enters a minimum funding of `0.01` CFX
- **THEN** the pricing step rejects the value and accepts only a minimum funding of `"0"`

### Requirement: Publishing requires an accepted share disclosure

Before publishing, the wizard SHALL show the exact terms to be published: model ID, revision, quantization, context and output limits, capabilities, serving profile and qualification identity (or, for a share without one, the label **Unqualified — no speed guarantee** and the statement that no serving profile is published; for an embedding share, that no generation serving profile applies), token rates, minimum funding, free allowance, fee rate and platform wallet, maximum concurrency, the transport (libp2p only, with no HTTPS endpoint) and the deployment's peer ID. It SHALL also show the pricing mode: with Auto-price, its target, floor and step prices; with manual pricing, the manual price the user recorded. It SHALL also show a disclosure that the user must actively accept. The disclosure SHALL state at least that:

- "consumers will prepay non-refundable balances that only this machine can serve", in the book's words (*Share online* step 6), followed by the statement that this applies once the share charges; the disclosure SHALL make both statements for every share, free or priced, whatever its pricing mode; only free accounts, opened during the share's free periods (token rates and minimum funding all zero, including a zero-price embedding share), prepay nothing, and each free account is served only within the free allowance; a share without a serving profile is published with no speed class, labeled **Unqualified — no speed guarantee**, with no performance remedy or waiver;
- with Auto-price on, the share starts with a launch window of at least 24 hours of free service, which lasts until it is ready to charge: the catalog reports its launch criteria met and the share has a valid qualification;
- when a qualification budget is approved, qualification runs automatically in the share's first quiet hour, or, on a busy share, about 12 hours after publication, later only when a stated deferral reason holds, spending up to that budget; the share pauses for the run, withdrawing its offers and answering consumers with a retryable busy response, and the pause counts as unavailable in its launch reputation; a failed run leaves the share free; after a passing run the share restarts with its qualified serving profile, which it keeps at every price, free included, and free accounts opened before that under its unqualified free offers keep being served under their allowance, with the same model and settings and no speed promise;
- with Auto-price on, prices for new accounts change automatically, by at most one step every 6 hours, between the floor and the target, from one catalog's launch reputation and payment-gated score; existing accounts keep their agreed terms;
- going offline (sleep, quit or network loss) makes the provider unavailable, and its offers expire within minutes;
- stopping sharing does not erase existing balances or obligations;
- prompts and outputs are processed in plaintext on this machine;
- this machine's public IP address is advertised on the network and is visible to the consumers and relays it serves;
- reputation reflects only paying users' feedback, so free service earns no ratings (book *Author decisions* item 9): free sessions cannot vote;
- catalogs observe this machine's availability whether it is free or charging and, while it is free, probe it with small real requests from unlinkable identities, including a check that it reports prompt-token counts honestly; they publish a service score, which consumers see beside the payment-gated score and never blended into it, as defined by the launch reputation of the pending change `specify-provider-launch-reputation`; the launch reputation and the payment-gated reputation stay attached to the provider wallet, so a share from another wallet starts without them;
- the provider owes the 0.5% platform fee on each funding and pays remittance gas; the openings of a free share carry no funding and so create no fee;
- the daemon remits the deployment's fee batches automatically from the provider wallet, spending gas, when they reach 10^15 wei or the oldest obligation is 86,400 seconds old, including obligations from earlier shares while a free share runs;
- signed offers and qualification evidence are provider-attributed statements, not attestation;
- the model's license and redistribution terms are the user's responsibility;
- on a relayed path, the relay sees both peers' IP addresses and peer IDs, timing and traffic volume, but not content.

Publishing SHALL be available only when all of these hold:

- the disclosure is accepted;
- the final reachability verification has passed;
- for a share published at a nonzero price, which only *Share again* does, it has a valid qualification for its exact profile; for every share, the daemon's readiness probes have passed;
- the provider wallet has sufficient gas, as *The provider wallet holds gas for fee remittance* requires for this share;
- the daemon reports the model ready;
- when the deployment holds accounts the share will not serve, the user has acknowledged them.

Changing any shown term SHALL withdraw the acceptance. The desktop SHALL report the share as published only after the daemon's published-offer list contains its offers.

#### Scenario: Disclosure not accepted

- **WHEN** the user has not accepted the share disclosure
- **THEN** publishing is unavailable and no offer is announced

#### Scenario: Publication is confirmed from the daemon

- **WHEN** the user publishes
- **THEN** the Share page reports *published* only after the daemon's published-offer list includes the share's offers
- **AND** if the daemon publishes none, the page shows the daemon's reason instead

#### Scenario: Changed terms require new acceptance

- **GIVEN** an accepted disclosure
- **WHEN** the user changes the Auto-price floor or the free allowance
- **THEN** the acceptance is withdrawn and must be given again before publishing

#### Scenario: Free share terms and disclosure

- **GIVEN** a new generation share whose readiness probes have passed
- **WHEN** the user reaches the publish step
- **THEN** the terms show token rates and minimum funding of `"0"`, the free allowance, no serving profile, and the label **Unqualified — no speed guarantee**
- **AND** the disclosure states "consumers will prepay non-refundable balances that only this machine can serve", and that this applies once the share charges
- **AND** it states that free accounts opened during free periods prepay nothing, that its openings create no platform fee, and that no performance remedy or waiver applies

#### Scenario: The disclosure explains ratings, launch reputation and Auto-price

- **GIVEN** a new share with Auto-price on and an approved qualification budget, whose readiness has passed
- **WHEN** the user reaches the publish step
- **THEN** the disclosure states that reputation reflects only paying users' feedback and that free service earns no ratings, and that catalogs observe availability, probe the share while it is free and publish a service score beside the payment-gated score, never blended into it, attached to the provider wallet
- **AND** it states that prices for new accounts change automatically with Auto-price on, that qualification runs automatically and pauses the share, counting as unavailable, and that free accounts opened before a passing run's promotion keep being served under their allowance after it, with no speed promise

### Requirement: The provider daemon is a managed child with loopback administration

While sharing, while qualifying and while checking reachability, the desktop SHALL run `chiral_daemon --config <provider.json> --version-policy <offline-signed policy>` as a child process. It SHALL run only the `chiral_daemon` executable shipped in its own installation. It SHALL locate that executable natively, SHALL NOT resolve it from `PATH`, a profile or user input, and SHALL refuse to start it if it reports a version other than the desktop's own.

The desktop SHALL generate the daemon configuration, the administrator and engine tokens and the provider DHT identity file under the app's protected `network/` directory. It SHALL set the configuration's `data_dir` to the deployment's `data_dir` base (see *Provider identity and wallet share one data_dir journal*), under which the daemon keeps the shared wallet journal and the deployment's ledger, lock and offer-revision state. That configuration SHALL bind administration to a nonzero literal loopback port, the managed engine to literal loopback, and the provider's HTTP listener to loopback, all on distinct ports. The published deployment's model SHALL carry no serving profile until a passing run is promoted into it (*A passing run is promoted into the published deployment in place*), and from then on the qualified serving profile and its evidence file at every price, free included. An embedding share's model carries none. While a free generation share's model has no serving profile, its published deployment SHALL rely on the free-generation exception of `inference-commercial-terms` (*Native deployment constraints cannot be bypassed by declaration validity*), never on development mode. Beyond the options this requirement names, the managed daemon SHALL support the qualification pause of *Qualification runs automatically in the share's first quiet hour* and the repricing action of *A running share is repriced by a new signed offer revision*, both as operator-only actions on the loopback administration listener.

For the published deployment and for the reachability-check daemon that runs under its identity, the configuration's DHT `listen` address SHALL be on a non-loopback interface, so that the provider can be dialed directly and can take part in hole punching, while its HTTP, administration and engine listeners stay on literal loopback. The calibration deployment's DHT listener stays on loopback as *Paid generation shares require in-app qualification within an approved budget* requires. The configuration of both of these daemons SHALL advertise no HTTPS origin (the provider's advertised public endpoint is `null`), and SHALL include the `libp2p_serving` block that `p2p-provider-transport` defines (*Provider libp2p serving is configured explicitly on the persisted deployment identity*):

- `relays` SHALL be copied from the provider profile, with 1–32 entries;
- each stream-server limit (the total and per-peer inbound stream limits, the request-head deadline, the maximum request-head size, the stream idle timeout and the maximum number of requests per stream) SHALL be set to a nonzero value by the desktop's stated rule for that limit, and SHALL NOT be omitted or left to a daemon default;
- the Share page SHALL show the resulting values before publishing.

The desktop SHALL generate the administrator and engine tokens natively as protected files of at least 32 random characters. It SHALL pass only file paths to the daemon, and SHALL NOT put a secret on a command line, in an environment variable, in a URL, in a log or in webview state. The desktop SHALL read the administrator token natively for every administration call, over loopback.

The child SHALL receive a minimal environment with no wallet credentials, proxy settings or library-injection variables. Every daemon the desktop starts, including calibration and reachability-check daemons, SHALL be started with `--version-policy` naming an authority-signed policy that the desktop has natively verified (signature, validity time and rollback watermark). Development mode SHALL NOT exempt a desktop-managed daemon from this. The daemon SHALL use the same effective policy authority as the desktop. When the desktop's authority comes from the `CHIRAL_POLICY_PUBLIC_KEY` override, that variable, which holds a public key, SHALL be passed to the child explicitly; otherwise it SHALL be absent from the child's environment.

A published deployment SHALL start without publishing offers until the desktop resumes it after the publish checks pass. If a configured port is occupied or the deployment's ledger lock is held, the desktop SHALL refuse to start and report the error, and SHALL NOT adopt, connect to or terminate the other process.

The child's lifetime SHALL be bound to the desktop: parent-death termination on Linux, a kill-on-close job object on Windows, and a stop of the child's process group on a normal macOS shutdown. An application quit request that the desktop can intercept SHALL be handled as `desktop-window-lifecycle` specifies (*Quitting is an explicit, confirmed action*). Some terminations cannot be intercepted but can be observed: operating-system shutdown, restart or session end, and termination signals. For these, the native exit handler SHALL drain and gracefully stop every share and qualification daemon without the liability display, and SHALL NOT block the shutdown.

On start, the desktop SHALL detect a daemon left running by an abrupt termination (a held lock or an occupied port) and SHALL tell the user instead of starting a second one. If the child exits unexpectedly, the desktop SHALL show the failure and SHALL NOT restart it automatically. An explicit restart SHALL reuse the same identity and state, and the dashboard SHALL then show the requests the daemon recovered under the unknown-usage policy.

#### Scenario: The administrator token never reaches the webview

- **WHEN** the Share dashboard reads status, accounts or fees
- **THEN** the native layer reads the token file and calls the loopback administration listener
- **AND** no token value appears in a webview message, command argument, environment variable or log

#### Scenario: A share's configuration is complete

- **GIVEN** a share whose provider profile lists two relays
- **WHEN** the desktop generates the published deployment's configuration
- **THEN** it advertises no HTTPS origin and contains a `libp2p_serving` block with those two relays and a nonzero value, set by the desktop's stated rule, for each stream-server limit, and the Share page shows those values before publishing
- **AND** its DHT `listen` address is on a non-loopback interface while its HTTP, administration and engine listeners are on literal loopback

#### Scenario: Occupied port

- **GIVEN** another process holds the configured administration or engine port
- **WHEN** the desktop starts the provider daemon
- **THEN** it refuses with an error naming the port
- **AND** it does not connect to, adopt or terminate the other process

#### Scenario: Unexpected daemon exit

- **WHEN** the managed daemon exits without being asked to stop
- **THEN** the Share page shows the failure and the share as not serving
- **AND** the daemon is not restarted until the user restarts it explicitly

#### Scenario: Nothing is published before resume

- **GIVEN** the desktop has started the deployment that will publish
- **WHEN** the publish checks have not yet passed
- **THEN** no offer is published: the daemon's published-offer list is empty and its manifest replies carry no offers

#### Scenario: Calibration without a verified policy is refused

- **GIVEN** a qualification run whose policy file is missing, has an invalid signature, has expired, or is older than the persisted rollback watermark
- **WHEN** the desktop would start the calibration daemon
- **THEN** it refuses to start the daemon and shows the policy error
- **AND** no qualification request is sent

#### Scenario: Only the bundled daemon runs

- **GIVEN** a different `chiral_daemon` executable earlier on `PATH`
- **WHEN** the desktop starts a share
- **THEN** it runs the `chiral_daemon` shipped in its own installation, and refuses to start if that executable reports a different version

#### Scenario: Operating-system shutdown while sharing

- **GIVEN** an active share
- **WHEN** the operating system shuts down or ends the user session
- **THEN** the desktop does not block the shutdown
- **AND** the native exit handler drains the share and asks the daemon to stop gracefully

### Requirement: The Share dashboard reports provider state through native administration

While a share exists, the Share page SHALL show, from the managed daemon's authenticated administration API called natively:

- liveness, serving readiness and funding readiness, as separate facts, and any chain error;
- the share's mode: **Free** when its input price, output price and minimum funding are all zero, and **Paid** otherwise; the pricing mode, Auto-price or manual, with the Auto-price state that *Auto-price sets the price from launch reputation* requires; whether the share is in its launch window; and the free allowance;
- the qualification state: no budget approved, a run scheduled (with its approved budget and the earliest time it may start), paused and running, passed and promoted, promotion failed, failed, or stopped, with any deferral reason;
- admission state (accepting, draining or paused for qualification), per-model readiness, and, for a share whose model carries a serving profile, its speed class and any durable performance-withdrawal latch, whatever its price; for a share whose model has none, the label **Unqualified — no speed guarantee** with the statement that it has no serving profile, speed class, performance remedy or waiver;
- the transport, libp2p, and the active path: direct, hole-punched or relayed, and which relay;
- published offers with their expiry;
- each account's funded, fee, spent, reserved and available amounts and status, plus the exact total of available and reserved balances as this provider's outstanding prepaid liability;
- request metadata only, never content;
- fee obligations by status;
- earnings: total accepted funding and total spent, labeled as not profit while prepaid balances remain owed;
- the provider wallet's gas balance.

It SHALL offer drain, resume, offer refresh and *Remit now*; the qualification budget approval, *Qualify now*, *Cancel scheduled qualification* and *Stop qualification* where *Paid generation shares require in-app qualification within an approved budget*, *Qualification runs automatically in the share's first quiet hour* and *A qualification run can be stopped explicitly* allow them; and the Auto-price checkbox, its target and floor, the free allowance and manual prices, as *Auto-price sets the price from launch reputation*, *Manual pricing applies when Auto-price is off* and *Free offers carry a signed free allowance* allow. The page SHALL state that the managed daemon remits fee batches automatically, sending provider-wallet transactions that spend gas, when a platform wallet's pending obligations reach 10^15 wei or the oldest reaches 86,400 seconds, including while it drains. *Remit now* forces remittance below those thresholds (`fees/remit`), and SHALL require confirmation that it sends a chain transaction that spends gas. The page SHALL state that a not-ready model does not admit requests, that existing accounts keep the prices and revisions they accepted, and that the dashboard installs nothing and exposes no shell; for a share whose model carries a serving profile it SHALL also state that resume and refresh do not clear a performance-withdrawal latch. For a share in Free mode, the fee section SHALL state that the share's own openings create no fee obligation and that automatic remittance applies only to the deployment's existing obligations.

When the provider profile names a catalog, the dashboard SHALL show this provider's launch reputation, as defined by the pending change `specify-provider-launch-reputation`, and its payment-gated reputation, exactly as that catalog reports them: the service score, the 14-day availability, the probe count, probe success rate and the age of the latest probe results, any *Billing mismatch* flag with its end, or **Billing unchecked** when the catalog reports that it could not perform the billing check (shown as neither a mismatch nor a blocked launch criterion), the date sharing began, each launch criterion, met or unmet, and the payment-gated score with its vote counts, with the catalog's origin and computation time. Beside them, and labeled as the desktop's own check rather than the catalog's, it SHALL show whether a generation share has a valid qualification; for an embedding share it SHALL state that no qualification applies, so the share is never ready to charge and Auto-price is unavailable for it. It SHALL read them natively, only from the provider profile's `catalog_url`, never from a catalog discovered from peers. It SHALL label the launch reputation as that catalog's observation and show it beside the payment-gated score, never blended into it. The desktop SHALL NOT compute, estimate or fabricate a launch reputation, and SHALL NOT sample or probe its own provider for one. When the catalog reports no launch reputation for this provider, or cannot be reached, the dashboard SHALL say so instead of showing a figure.

Book *Author decisions* item 6 gives a relayed share a 24-hour grace after its offers lapse. Under book *Relays carry ciphertext, under explicit limits*, a serving relay, as `p2p-provider-transport` specifies it, keeps granting serving-grade circuits to this machine's peer ID, within the same per-provider quotas, until 24 hours after the signed `expires_at` of the most recent offer from this machine that the relay itself verified. The *relayed-grace statement* SHALL say that:

- while this machine's daemon is running without a current offer, serving relays keep carrying consumers' request status, receipts, cancellation and use of existing accounts until 24 hours after this machine's most recent offer verified by the relay expires;
- after that, consumers may lose access to them through the relay until this machine publishes a valid offer again, unless a direct or hole-punched path is established;
- a relay whose evidence store is full or has failed can end the grace earlier, because `p2p-provider-transport` bounds a relay's durable grace evidence by an operator-configured relay-wide quota, and fails closed for grace when that store fails;
- neither the grace nor its end changes any balance, obligation or account: only the relayed path is affected, and existing accounts stay recoverable when any path to this machine's daemon is available again.

Before draining a share on a relayed path, the drain action SHALL give the relayed-grace statement; so SHALL the qualification step and *Qualify now* for a share on a relayed path, because the qualification pause withdraws its offers. While a share on a relayed path has no current published offer, the dashboard SHALL show the relayed-grace statement together with the signed `expires_at` of the last offer the desktop observed in the daemon's published-offer list and the time 24 hours after it. That time SHALL be labeled as the latest time the grace can end: a relay that verified only an earlier offer, or whose evidence store is full or has failed, ends it sooner. This includes while the share drains, while it is paused for qualification or being promoted, while offer publication is paused during a chain outage and, for a share whose model carries a serving profile, while a performance-withdrawal latch withdraws its model.

Amounts SHALL be converted from exact wei without floating point. Provider metadata and responses SHALL render as plain text. Status changes SHALL be announced with `role="status"`, failures with `role="alert"`, and every status badge SHALL carry text. The existing Provider page for externally operated providers SHALL remain available unchanged.

#### Scenario: Funding readiness shown separately

- **GIVEN** the chain RPC is temporarily unavailable while serving remains healthy
- **WHEN** the dashboard refreshes
- **THEN** it shows serving as ready and new funding as not ready, with the chain error

#### Scenario: Forced remittance needs confirmation

- **WHEN** the user chooses *Remit now*
- **THEN** a confirmation states that a chain transaction will be sent and gas spent
- **AND** nothing is forced unless the user confirms

#### Scenario: Automatic remittance is disclosed

- **GIVEN** an active share
- **WHEN** the user views the fee section of the dashboard
- **THEN** it states that the daemon remits automatically from the provider wallet at 10^15 wei or after 86,400 seconds, spending gas

#### Scenario: Resume does not clear withdrawal

- **GIVEN** a model with a performance-withdrawal latch
- **WHEN** the user resumes admission or refreshes offers
- **THEN** the model remains withdrawn and the dashboard says so

#### Scenario: Free share on the dashboard

- **GIVEN** an active free generation share without a serving profile, in its launch window, with a qualification run scheduled
- **WHEN** the user views the dashboard
- **THEN** it shows the mode **Free**, Auto-price on with the launch window, the free allowance, the label **Unqualified — no speed guarantee** and no speed class or performance-withdrawal latch
- **AND** it shows the scheduled run with its approved budget and earliest start, offers *Qualify now* and *Cancel scheduled qualification*, and its fee section states that the share's openings create no fee obligation

#### Scenario: Paid share on the dashboard

- **GIVEN** an active generation share at the Auto-price 50% step, with a promoted qualification
- **WHEN** the user views the dashboard
- **THEN** it shows the mode **Paid** with the share's speed class, the current step and the time of the next review
- **AND** it shows the last review's decision with its reason and inputs as the catalog reported them

#### Scenario: Launch reputation is shown only as the catalog reports it

- **GIVEN** an active share whose provider profile names a catalog that reports no launch reputation for this provider
- **WHEN** the dashboard shows the launch reputation
- **THEN** it states that the catalog reports no launch reputation yet, and shows no service score, availability figure or sharing date

#### Scenario: An unchecked billing check is not a mismatch

- **GIVEN** an active free share whose catalog reports its billing check as unchecked because it could not obtain the declared tokenizer
- **WHEN** the dashboard shows the launch reputation
- **THEN** it shows **Billing unchecked**, no *Billing mismatch* flag, and no unmet launch criterion for billing

#### Scenario: Exact liability total

- **WHEN** two accounts have available balances of `1` and `999999999999999999` wei and nothing reserved
- **THEN** the outstanding prepaid liability is shown as exactly `1` CFX

#### Scenario: Draining a relayed share gives the relayed-grace statement

- **GIVEN** an active share whose active path is relayed
- **WHEN** the user chooses drain on the dashboard
- **THEN** before draining, the dashboard states that while the daemon runs, serving relays keep carrying request status, receipts, cancellation and existing accounts until 24 hours after this machine's most recent offer verified by the relay expires, that a relay whose evidence store is full or has failed can end that earlier, and that after that consumers may lose access to them through the relay unless a direct or hole-punched path is established
- **AND** while the share has no current published offer, the dashboard keeps showing that statement with the `expires_at` of the last offer the daemon published and, 24 hours later, the latest time the grace can end

### Requirement: Local use of a shared model passes through admission

While a model is shared, local chat on it, and local API server requests routed to it once that server exists, SHALL be dispatched through the managed daemon's admission. Each such request SHALL acquire, before dispatch, one of the model's advertised concurrency slots for its duration and, when the shared model carries a serving profile, a share of the same qualified arrival budget, whatever the share's price, and SHALL be subject to the same request validation and bounds as network traffic. A share whose model has no serving profile has no qualified arrival budget; local requests to it acquire only a slot. Local use SHALL NOT run on a second engine instance or reach the engine directly. When every slot is occupied, the qualified arrival budget is exhausted, admission is draining or paused for qualification, or the model is not ready, a local request SHALL be refused with the reason; it SHALL NOT be queued ahead of network traffic. Local requests SHALL be dispatched only through an operator-authenticated route on the loopback administration listener, never through the public listener or a libp2p stream. A local request SHALL NOT debit, credit or create a prepaid account, create a fee obligation or reputation event, or enter the paid performance observations or waiver accounting. The chat view of `desktop-local-chat` SHALL show that the model is shared and that local use occupies a slot. Output SHALL render as untrusted plain text, and cancellation SHALL propagate to the daemon.

This is the dispatch that `desktop-local-chat` delegates to this capability for an installation that backs an active share. For such an installation:

- sending in a local thread SHALL be enabled without a local load;
- the native chat command SHALL send the thread's content only to this loopback route, which passes it only to the share's managed engine on this machine, and SHALL use no per-load engine key;
- a refused request SHALL be shown with its admission reason (saturated, arrival budget exhausted for a share with a serving profile, draining, paused for qualification or not ready) instead of a local busy state;
- *Stop* SHALL end the generation through the daemon;
- token counts SHALL be those the engine reports, relayed by the daemon, and SHALL NOT be estimated from text.

While a qualification run of an installation is in progress, local chat and local API server requests on it SHALL be refused with that reason, because they would disturb the measurement.

#### Scenario: Shared installation needs no local load

- **GIVEN** an installation that backs an active share and is not loaded in a local engine
- **WHEN** the user selects it in a local chat thread
- **THEN** it is labeled as shared and sending is enabled
- **AND** a sent message reaches the daemon's local admission route and no engine is contacted directly

#### Scenario: No local use during qualification

- **GIVEN** a qualification run of an installation in progress
- **WHEN** the user sends a local chat message on that installation
- **THEN** sending is refused with an explanation that the installation is being qualified

#### Scenario: No local API use during qualification

- **GIVEN** a qualification run of an installation in progress and the local API server enabled
- **WHEN** an SDK client sends a request on a local route to that installation
- **THEN** the request is refused without dispatch, with an explanation that the model is being qualified

#### Scenario: Local chat occupies a slot

- **GIVEN** a shared model with maximum concurrency 1 and a local chat request in progress
- **WHEN** a paid request arrives
- **THEN** the paid request is refused as saturated rather than run alongside it

#### Scenario: Local chat on a share without a serving profile takes only a slot

- **GIVEN** a free share whose model has no serving profile, with maximum concurrency 2 and one free network request in progress
- **WHEN** the user sends a local chat message on that model
- **THEN** the message takes the remaining slot and is dispatched without any arrival-budget check
- **AND** a further network request is refused as saturated until a slot is released

#### Scenario: Saturated share refuses local chat

- **GIVEN** every slot of a shared model is held by paid requests
- **WHEN** the user sends a local chat message
- **THEN** the message is refused with a saturation reason and is not queued

#### Scenario: Local use changes no money

- **WHEN** a local chat request on a shared model completes
- **THEN** no prepaid account, fee obligation, reputation event or performance observation is created or changed

### Requirement: Closing the window keeps sharing running

A close request on the main window SHALL NOT drain, stop, pause or withdraw an active share, and SHALL NOT interrupt a qualification run. Scheduled qualification runs, promotions and Auto-price reviews SHALL run natively and SHALL continue while the window is closed. The status bar that `desktop-local-workbench` defines (*The status bar shows local engine and download state*) SHALL show, on every page, the sharing state (sharing, draining, stopped with outstanding liabilities, or failed) and the path (direct, hole-punched or relayed). Logging out or loading a different profile SHALL be refused until stopping completes: while a share is active, with an explanation that points to *Stop sharing*; while a qualification run is in progress, with an explanation that points to *Stop qualification*. A share SHALL NOT end without the drain and the liability display, except through the terminations that cannot be intercepted named in *The provider daemon is a managed child with loopback administration*.

#### Scenario: Close keeps the share serving

- **GIVEN** an active share serving a paid request
- **WHEN** the user closes the main window
- **THEN** the window is minimized and the request, admission and published offers are unaffected

#### Scenario: Logout while sharing

- **GIVEN** an active share
- **WHEN** the user chooses Log out
- **THEN** logout is refused with an explanation that sharing must be stopped first
- **AND** the share keeps running

#### Scenario: Indicator on every page

- **GIVEN** an active share over a relayed path
- **WHEN** the user navigates to any page
- **THEN** the status bar shows that sharing is active and that the path is relayed

### Requirement: Idle system sleep is prevented automatically while sharing, with consent

Sleep prevention SHALL be automatic while sharing, once the user has consented. The wizard SHALL ask for consent with the option *Prevent idle system sleep while sharing* preselected. It SHALL show the option before the first action that would hold prevention: in the qualification step before a budget is approved, for a share that qualifies, and in the publish step, for every share. Proceeding with the option selected SHALL record consent; proceeding with it cleared SHALL record that consent was declined. The option SHALL be preselected each time the wizard asks, including after an earlier decline or revocation; a revocation SHALL stay in effect until the user next proceeds through the wizard with the option selected. Consent SHALL be a locally stored native setting, revocable at any time in Settings and on the Share dashboard.

With consent, the desktop SHALL prevent idle system sleep automatically whenever a share is active or a qualification run is in progress, without a further prompt. It SHALL release the prevention when sharing stops and no qualification run is in progress, when a run ends and no share is active, and when the daemon fails, the desktop quits or consent is revoked. It SHALL prevent only idle system sleep, and SHALL NOT prevent lid-close sleep, user-initiated sleep, shutdown or display sleep. It SHALL use the platform's native power-management interface and SHALL NOT need a webview permission for this. The desktop SHALL NOT prevent system sleep without consent. Without consent, the Share page SHALL state that system sleep makes the provider unavailable and that its offers expire within minutes. When the desktop detects that the system slept while sharing, the dashboard SHALL state that the provider was unavailable during that interval and that its offers may have expired.

#### Scenario: Consent is asked with the option preselected

- **GIVEN** any user, including one who declined or revoked sleep-prevention consent earlier
- **WHEN** the user reaches the publish step
- **THEN** the option *Prevent idle system sleep while sharing* is shown selected
- **AND** publishing with it selected records consent, and idle system sleep is prevented while the share is active

#### Scenario: No consent, no prevention

- **GIVEN** an active share whose wizard was completed with the option cleared
- **WHEN** the system becomes idle
- **THEN** the desktop does not hold a sleep-prevention assertion
- **AND** the Share page states that system sleep makes the provider unavailable

#### Scenario: Consent applies only while sharing

- **GIVEN** recorded consent
- **WHEN** the share stops or the daemon fails
- **THEN** the desktop releases its sleep-prevention assertion

#### Scenario: Revoking consent

- **GIVEN** an active share with sleep prevention held
- **WHEN** the user revokes consent in Settings or on the Share dashboard
- **THEN** the assertion is released and the share keeps running

#### Scenario: A revocation lasts until consent is given again

- **GIVEN** a user who revoked consent on the Share dashboard during an active share
- **WHEN** that share keeps running, and later the user reaches the publish step of a new share
- **THEN** no prevention is held for the running share
- **AND** the new share's wizard shows the option preselected, and prevention is held for it only if the user proceeds with the option selected

#### Scenario: User-initiated and lid-close sleep still work

- **GIVEN** an active share with sleep prevention held
- **WHEN** the user chooses Sleep or Suspend from the operating system, or closes the lid of a laptop configured to sleep on lid close
- **THEN** the system sleeps

### Requirement: Stop sharing shows liabilities, then drains, withdraws and stops

*Stop sharing* SHALL first show the outstanding liabilities reported by the daemon and require confirmation. These are:

- accounts with nonzero available or reserved balances, and their exact total;
- funding intents accepted but not yet credited;
- in-flight requests;
- unremitted fee obligations.

When a qualification run is scheduled, the confirmation SHALL state that stopping lapses its budget approval; when a run is in progress, the confirmation SHALL include the in-flight qualification requests, and stopping SHALL first stop the run as *A qualification run can be stopped explicitly* requires. The confirmation SHALL state that stopping does not erase balances or obligations, that consumers cannot use their balances until this machine shares the same model, artifact and serving terms from this deployment again, and that offers already distributed expire within their lifetime. For a share on a relayed path, it SHALL also give the relayed-grace statement of *The Share dashboard reports provider state through native administration*: while the daemon drains and waits, serving relays keep carrying request status, receipts, cancellation and existing accounts until 24 hours after this machine's most recent offer verified by the relay expires; after that, consumers may lose access to them through the relay unless a direct or hole-punched path is established; and a relay whose evidence store is full or has failed can end the grace earlier. Once the daemon has stopped, the provider is unavailable on every path until this machine shares again. It SHALL offer forced fee remittance as an explicit choice, and SHALL NOT force a remittance (`fees/remit`) unless the user chooses it. Automatic aggregation continues while the daemon runs, including while it drains and waits. After confirmation the desktop SHALL:

1. drain the deployment, stopping new admission and funding readiness and withdrawing published offers;
2. wait for in-flight requests to settle, showing their count, until none remain or the user chooses to stop now, after a statement that interrupted requests settle under the known-usage or unknown-usage policy;
3. stop the daemon through its graceful shutdown;
4. report the share as stopped only after the child has exited.

The desktop SHALL preserve the provider identity, ledger, acknowledgements, journals, keys, evidence, offer revision, pricing mode, Auto-price settings and step, and review history, and SHALL release sleep prevention. If draining or stopping fails, it SHALL report the error, show the share as still running, and SHALL NOT claim that it stopped.

#### Scenario: Liabilities before stopping

- **GIVEN** an active share with an account holding available balance
- **WHEN** the user chooses Stop sharing
- **THEN** the account, the exact total liability and the unremitted fees are shown before any change is made

#### Scenario: Stopping a relayed share gives the relayed-grace statement

- **GIVEN** an active share whose active path is relayed
- **WHEN** the user chooses Stop sharing
- **THEN** the confirmation states that while the daemon drains and waits, serving relays keep carrying request status, receipts, cancellation and existing accounts until 24 hours after this machine's most recent offer verified by the relay expires, that a relay whose evidence store is full or has failed can end that earlier, and that after that consumers may lose access to them through the relay unless a direct or hole-punched path is established
- **AND** it states that once the daemon has stopped, the provider is unavailable on every path until this machine shares again

#### Scenario: Drain precedes shutdown

- **WHEN** the user confirms Stop sharing
- **THEN** the daemon's published-offer list is empty and admission is closed before the daemon is asked to shut down
- **AND** the provider identity and ledger remain on disk after it exits

#### Scenario: Stop failure is not reported as stopped

- **GIVEN** the daemon does not exit after the stop request
- **THEN** the Share page reports the failure and keeps showing the share as running

### Requirement: An active share pins its model and runtime

While a share is active or a qualification run is in progress, the desktop SHALL refuse to eject the shared model or to delete, replace or move the model artifact or runtime that the share or run uses, and SHALL explain that sharing must be stopped first or, when a qualification run is the cause, that the run must first be stopped with *Stop qualification*. Read-only re-verification MAY run. The My models list of `desktop-model-acquisition` SHALL show each installation's share status: active share, stopped share with outstanding liabilities, or none. Before deleting a model that backs a stopped share with outstanding prepaid liabilities, the desktop SHALL show those liabilities as last observed and SHALL state that existing accounts can be served only by this deployment with that exact artifact.

While a qualification run is in progress, the desktop SHALL refuse to load any local model, including the installation under qualification, and SHALL explain that a concurrent load would invalidate the measurement. While a share is active, the desktop SHALL refuse to load another local model whose devices overlap the share's devices, unless the user first acknowledges the consequences. On unified memory every local load overlaps. The acknowledgement SHALL state that the load competes for the share's memory and compute, and, for a share whose model carries a serving profile, whatever its price, that it can cause paid requests to miss the qualified class, waivers to consumers, and a durable performance withdrawal of the shared model that resume and refresh do not clear. For a share whose model has no serving profile, and so no speed class, it SHALL state that the load competes with the share's consumers for memory and compute. The acknowledgement SHALL be requested for each such load.

#### Scenario: Delete or eject refused while shared

- **GIVEN** an active share of a model
- **WHEN** the user tries to delete or eject that model
- **THEN** the action is refused with an explanation that sharing must be stopped first

#### Scenario: Deleting a model with outstanding liabilities

- **GIVEN** a stopped share of a model whose accounts still hold available balance
- **WHEN** the user tries to delete the model
- **THEN** the desktop shows the last observed liabilities and the consequence before asking for confirmation

#### Scenario: No local load during qualification

- **GIVEN** a qualification run in progress
- **WHEN** the user tries to load any local model, including the installation under qualification
- **THEN** the load is refused with an explanation that it would invalidate the measurement
- **AND** no desktop engine child starts

#### Scenario: Overlapping load while sharing needs acknowledgement

- **GIVEN** an active share on device `CUDA0`
- **WHEN** the user loads another local model on `CUDA0`
- **THEN** no engine starts until the user acknowledges that the load can cause paid misses, waivers and a durable withdrawal of the shared model
- **AND** declining leaves the other model unloaded and the share unchanged

### Requirement: Sharing keeps secrets native and adds no webview permission

The webview SHALL NOT receive the provider signing key, administrator token, engine token, RPC bearer, qualification credentials or any other secret used by sharing; the GUI SHALL pass only choices and file paths to native commands. Daemon supervision, qualification and sleep prevention SHALL be native. Sharing SHALL NOT add shell, process, filesystem, power-management or other plugin permissions to the webview capability; any new capability entry SHALL be limited to the desktop's own share commands. Diagnostics and logs produced for sharing SHALL NOT contain secrets, prompts or generated text.

#### Scenario: Capability file after sharing support

- **WHEN** the desktop capability file is inspected
- **THEN** it contains no shell, process, filesystem or power-management plugin permission
- **AND** its window permissions are unchanged
