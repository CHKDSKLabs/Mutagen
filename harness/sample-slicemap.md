# Slice Map

Before slicing, here are the things I'd flag. None are hard tech-stack breakers — all are tracked in your ISCs already — but they surface at slice boundaries and need an answer so I know which verification step passes:

  1. ISC-012 (high severity): PRD 8.3 says "no chemical data stored in plaintext on-device" but Firestore's Android SDK writes a plaintext LevelDB cache for
  offline persistence (ADR-006 depends on this). You either (a) accept the cache as an exception and document it, or (b) disable Firestore persistence and
  rebuild the write-queue mechanism yourself — which invalidates the "SDK handles offline for us" upside of ADR-006. I'll slice assuming (a) with documented
  threat-model note. Flag me if that's wrong.
  1. ISC-010: MAHC 2 delivery mechanism is unresolved. I'll slice an initial bundled-seed + Firestore system doc override strategy so the app can be patched
  remotely without a store update. If you want APK-bundled only, let me know.
  1. PRD open Q #1: MAHC 2 temperature ranges per facility type aren't compiled. Slice assumes temperature is captured but not status-compared until the table
  is finalized. A stub with a TODO pointer will live in the seed data slice.
  1. ADR-010 JUnit 5 vs Android instrumentation: JUnit 5 runs fine for JVM tests via android-junit5 plugin, but instrumentation tests (Room, Compose) must
  remain on JUnit 4. Slicing will reflect that split — just calling it out.

  Proceeding with slicing. Dependency rule is strict. MVP-critical slices are marked [Phase 1]; everything else maps to Phase 2-4 per PRD section 10.

  ---
  Layer 1 — Foundation

  [Slice ID: L1-01] - Android Project Scaffold

- Objective: Stand up the Gradle project with the full plugin and dependency manifest for the entire stack.
  - Target LOC: ~350
  - Context to Update: project_state.md → "Foundation / Project Setup"
  - Implementation Details:
    - Android Studio single-module project, minSdk = 26, targetSdk = 35, compileSdk = 35, Kotlin 2.x + KSP.
    - Version catalog at gradle/libs.versions.toml. Declare: Compose BOM, Material 3, Navigation Compose, Hilt + Hilt Navigation Compose, Room + Room-KTX + Room
   compiler (KSP), SQLCipher (net.zetetic:android-database-sqlcipher), Jetpack Security (androidx.security:security-crypto), Firebase BOM, Firebase Auth,
  Firestore, Functions, Messaging, Storage, Analytics, Play Services Auth, Google Play Billing v7+, WorkManager, DataStore, Coroutines, Kotlinx Serialization,
  Coil, JUnit 5, MockK, Turbine, Robolectric, android-junit5 plugin, Compose UI Test.
    - Apply Hilt plugin, google-services plugin, and android-junit5 plugin in app module. Configure testOptions { unitTests.all { useJUnitPlatform() } }.
    - Build variants: debug (no SQLCipher passphrase — optional unencrypted variant for DB Inspector), staging, release. applicationIdSuffix for debug/staging.
    - .gitignore for google-services.json, .idea, local.properties, etc.
  - Verification Step: ./gradlew assembleDebug succeeds and produces an APK.
  - Human Check Needed?: No.

  [Slice ID: L1-02] - Hilt Application + Single-Activity Scaffold

  - Objective: Create @HiltAndroidApp application class and a single MainActivity hosting a Compose Navigation root.
  - Target LOC: ~200
  - Context to Update: project_state.md → "Foundation / Runtime Entry"
  - Implementation Details:
    - AquaOpsApp class annotated @HiltAndroidApp. Register in manifest.
    - MainActivity annotated @AndroidEntryPoint, sets Compose content to AquaOpsApp() root composable.
    - Root composable calls a placeholder NavHost with a single start destination that renders "Hello AquaOps".
    - Package structure under com.chkdsklabs.aquaops: ui/, viewmodel/, domain/{model,usecase,repository}, data/{local,remote,repository}, di/, util/.
    - Empty Hilt modules: DatabaseModule, NetworkModule, RepositoryModule, UseCaseModule, AuthModule, BillingModule.
  - Verification Step: Install debug APK on emulator; app launches and shows "Hello AquaOps" without crash. ./gradlew :app:compileDebugKotlin passes.
  - Human Check Needed?: No.

  [Slice ID: L1-03] - Theme + Design System Tokens

  - Objective: Encode the entire AquaOps design style guide as Compose Material 3 theme primitives.
  - Target LOC: ~450
  - Context to Update: project_state.md → "Foundation / Design System"
  - Implementation Details:
    - ui/theme/Color.kt: every color from design-style §2.1, §2.2, §2.3, §2.4 as Color(0xFF...) constants. Separate LightColors and DarkColors ColorScheme
  objects mapping to Material 3 roles, plus a StatusColors data class extension (fernGreen, goldenrodYellow, brickRed, plus tint variants) exposed via
  CompositionLocalProvider.
    - ui/theme/Type.kt: Typography with the nine roles from §3.1 (Display Large through Label Small) using Roboto, sp sizes, weights, line-heights, tracking.
    - ui/theme/Shape.kt: Material 3 shapes (small=8dp, medium=12dp, large=16dp).
    - ui/theme/Spacing.kt: data class + LocalSpacing composition local with tokens xs, sm, md, base, lg, xl, xxl.
    - AquaOpsTheme composable wraps MaterialTheme with dynamic color disabled (per §10). Honors system dark mode.
    - Unit test: ThemeTest verifies both themes instantiate without exception.
  - Verification Step: ./gradlew :app:testDebugUnitTest --tests "*ThemeTest*" green.
  - Human Check Needed?: No.

  [Slice ID: L1-04] - Firebase Integration + Emulator Suite

  - Objective: Wire the Android app to Firebase and stand up the local emulator suite for dev/test.
  - Target LOC: ~250
  - Context to Update: project_state.md → "Foundation / Firebase"
  - Implementation Details:
    - Create Firebase project "aquaops-dev" + "aquaops-prod" in Firebase Console (manual — document in README). Drop google-services.json per flavor in
  app/src/{debug,staging,release}/.
    - Initialize Firebase: Auth, Firestore, Storage, Functions, FCM, Analytics in AquaOpsApp.onCreate.
    - Configure Firestore: FirebaseFirestore.getInstance().firestoreSettings = FirestoreSettings.Builder().setPersistenceEnabled(true).setCacheSizeBytes(100 *
  1024* 1024).build() — document ISC-012 exception in code comment referencing threat model.
    - firebase/ directory at repo root containing firebase.json, firestore.rules (empty allow-deny for now), firestore.indexes.json (empty), storage.rules,
  emulator config (ports 9099 auth, 8080 firestore, 5001 functions, 9199 storage, 8085 pubsub).
    - Debug build detects emulator via BuildConfig.USE_EMULATOR and calls useEmulator(...) on each SDK.
  - Verification Step: firebase emulators:start runs; debug APK launches against emulator and successfully writes a test document to Firestore via a scratch
  button (remove button after verify).
  - Human Check Needed?: Yes — requires Firebase Console project creation credentials.

  [Slice ID: L1-05] - CI Pipeline (GitHub Actions)

  - Objective: Continuous integration for every push: lint, unit tests, build.
  - Target LOC: ~150
  - Context to Update: project_state.md → "Foundation / CI"
  - Implementation Details:
    - .github/workflows/android.yml: Runs on push + PR to main. Ubuntu latest. JDK 17. Gradle cache action. Steps: ./gradlew ktlintCheck detekt
  :app:testDebugUnitTest :app:assembleDebug.
    - .github/workflows/functions.yml: Scaffold (Cloud Functions come online in L4) — runs npm ci && npm run lint && npm test in firebase/functions/. For now
  it's a no-op placeholder.
    - Add ktlint and detekt Gradle plugins with baseline configs (detekt.yml, .editorconfig).
    - PR template at .github/PULL_REQUEST_TEMPLATE.md with Test Plan checklist.
  - Verification Step: Push a commit to a feature branch; GitHub Actions run goes green.
  - Human Check Needed?: Yes — requires repo admin to confirm branch protection rules.

  ---
  Layer 2 — Data

  [Slice ID: L2-01] - SQLCipher Key Management + Room Database

  - Objective: Implement the Android Keystore-backed SQLCipher passphrase flow and the empty Room database container.
  - Target LOC: ~400
  - Context to Update: project_state.md → "Data / Local Storage" (document Keystore alias aquaops_db_key, ISC-003)
  - Implementation Details:
    - data/local/security/DatabaseKeyProvider.kt: Generates a 256-bit random key on first access, stores it in Keystore via EncryptedSharedPreferences under
  alias aquaops_db_key. Retrieval returns ByteArray. Handle KeyStoreException with explicit KeystoreUnavailable sealed result.
    - data/local/AquaOpsDatabase.kt: @Database class annotated with empty entities = [] for now (entities added in L2-02). Singleton builder uses
  SupportFactory(key) with passphrase from DatabaseKeyProvider.
    - DatabaseModule: @Provides @Singleton fun provideDatabase(app, keyProvider): AquaOpsDatabase. Debug variant bypasses SQLCipher if
  BuildConfig.DEBUG_UNENCRYPTED_DB is true (ADR-004 mitigation for DB Inspector).
    - Instrumentation test DatabaseKeyProviderTest (androidTest): key generation is deterministic across calls, survives app process restart.
  - Verification Step: ./gradlew :app:connectedDebugAndroidTest --tests "*DatabaseKeyProviderTest*" green on an emulator.
  - Human Check Needed?: No.

  [Slice ID: L2-02] - Room Entities + Type Converters

  - Objective: Define every Room entity from the DDD and PRD data model with full typed columns.
  - Target LOC: ~500
  - Context to Update: project_state.md → "Data / Entities" (list every entity + field)
  - Implementation Details:
    - data/local/entity/PoolEntity.kt: id (UUID PK), ownerId, name, volumeAmount, volumeUnit, poolType, surfaceType, sanitizerSystem, createdAt, updatedAt,
  syncedAt.
    - LogEntryEntity.kt: id (UUID PK, indexed by poolId + testedAt), poolId FK, recordedBy, testedAt, freeChlorine?, combinedChlorine?, ph?, totalAlkalinity?,
  calciumHardness?, cyanuricAcid?, tds?, waterTemp?, tempUnit, notes, createdAt, updatedAt, syncedAt. No totalChlorine column (ISC-011, derived).
    - ChemicalStandardEntity.kt: id, ownerId?, name, isSystem, rangesJson (stored as string), createdAt, updatedAt, syncedAt.
    - PoolMembershipEntity.kt: composite PK (poolId, userId), role, state, invitedAt, acceptedAt?, syncedAt.
    - SubscriptionEntity.kt: userId PK, state, plan?, purchaseToken?, currentPeriodStart?, currentPeriodEnd?, graceDeadline?, verifiedAt?.
    - ReagentTrackerEntity.kt: id PK, poolId FK, reagentName, totalTests, remainingTests, alertThreshold, supplierUrl?, syncedAt.
    - AlertConfigEntity.kt: composite PK (poolId, alertType), enabled, quietHoursStart?, quietHoursEnd?, testReminderIntervalHours?, syncedAt.
    - data/local/converter/Converters.kt: UUID ↔ String, Instant ↔ Long, every enum ↔ String, Map<Parameter, ParameterRange> ↔ JSON via kotlinx.serialization.
    - Register @TypeConverters(Converters::class) on the database.
    - Unit tests for every converter (ConvertersTest).
  - Verification Step: ./gradlew :app:testDebugUnitTest --tests "*ConvertersTest*" green and ./gradlew :app:kaptDebugKotlin :app:kspDebugKotlin compiles (Room
  schema generation passes).
  - Human Check Needed?: No.

  [Slice ID: L2-03] - Room DAOs + Flow Queries

  - Objective: DAO interfaces for all entities with Flow-based reads (per ISC-001).
  - Target LOC: ~450
  - Context to Update: project_state.md → "Data / DAOs" (list every DAO method)
  - Implementation Details:
    - PoolDao: observeAll(ownerId), observeById(id), upsert(pool), delete(id), countByOwner(ownerId), observePendingSync() (where syncedAt IS NULL).
    - LogEntryDao: observeByPool(poolId), observeByPoolAndDateRange(poolId, from, to), searchByNotes(poolId, query), observeMostRecent(poolId),
  observeLastN(poolId, n), observeAllAcrossPools(poolIds), upsert, delete(id), observePendingSync().
    - ChemicalStandardDao: observeSystemStandards() (isSystem = 1), observeByOwner(ownerId), getById, upsert, delete.
    - PoolMembershipDao: observeByUser(userId), observeByPool(poolId), upsert, delete.
    - SubscriptionDao: observe(userId), upsert(subscription).
    - ReagentTrackerDao, AlertConfigDao: corresponding Flow + CRUD methods.
    - DatabaseModule provides each DAO via @Provides.
    - Room schema exported to schemas/ (for migration diffing).
    - Instrumentation tests for each DAO covering one happy-path query + upsert + sync-null filter.
  - Verification Step: ./gradlew :app:connectedDebugAndroidTest --tests "*DaoTest*" green.
  - Human Check Needed?: No.

  [Slice ID: L2-04] - Seed Data Migration (MAHC 2)

  - Objective: On first open, seed the database with the MAHC 2 system standard so the free tier works offline immediately.
  - Target LOC: ~250
  - Context to Update: project_state.md → "Data / Seed" (include MAHC 2 range table verbatim)
  - Implementation Details:
    - data/local/seed/Mahc2Seed.kt: static ChemicalStandardEntity with isSystem = true, ranges for free chlorine (1.0–10.0), combined chlorine (–, 0.4), pH
  (7.2–7.8), total alkalinity (60–180), calcium hardness (150–1000), cyanuric acid (–, 90). Temperature parameter included but marked null range with //
  TODO(ISC-010): MAHC 2 temp table pending.
    - RoomDatabase.Callback.onCreate inserts the seed standard. On onOpen, reconcile: if the seed's version column differs from bundled, re-upsert (idempotent).
    - Add version: Int column to ChemicalStandardEntity for update mechanism (ISC-010).
    - Unit test: open in-memory DB, assert MAHC 2 standard exists with correct ranges.
  - Verification Step: ./gradlew :app:connectedDebugAndroidTest --tests "*SeedTest*" green.
  - Human Check Needed?: Yes — confirm MAHC 2 temp table owner is still Product (PRD open Q#1) before launch.

  [Slice ID: L2-05] - Firestore DTOs + Mappers

  - Objective: Serializable DTOs mirroring the Firestore document shapes from ADR-005, with bidirectional mappers to domain models.
  - Target LOC: ~400
  - Context to Update: project_state.md → "Data / Firestore DTOs"
  - Implementation Details:
    - data/remote/dto/PoolDto.kt, LogEntryDto.kt, ChemicalStandardDto.kt, PoolMembershipDto.kt, SubscriptionDto.kt, ReagentTrackerDto.kt — all @Serializable,
  @DocumentId for the id field.
    - Mappers in data/remote/mapper/FirestoreMappers.kt: PoolEntity.toDto(), PoolDto.toEntity(), etc. Timestamps use Firestore Timestamp.
    - Critical: On writes, set updatedAt = FieldValue.serverTimestamp() (ISC-006). On pull sync, copy server-assigned timestamp back to the Room entity.
    - LogEntry DTO path: pools/{poolId}/logs/{logId} — mapper knows subcollection path.
    - Unit tests for every mapper (roundtrip: entity → DTO → entity equals original modulo syncedAt).
  - Verification Step: ./gradlew :app:testDebugUnitTest --tests "*FirestoreMappersTest*" green. (Note: original filter `*MapperTest*` was a typo — missed the trailing `s` in `Mappers`.)
  - Human Check Needed?: No.

  [Slice ID: L2-06] - Firestore Security Rules + Indexes

  - Objective: Enforce tenancy and inherited-premium access at the database level. (Original L2-06 shipped the bilateral-premium model; superseded by L5-13-0 Path B — helpers renamed to ownerHasActivePremium / canAccessSharedPool.)
  - Target LOC: ~350
  - Context to Update: project_state.md → "Data / Security Rules"
  - Implementation Details:
    - firebase/firestore.rules:
      - match /users/{uid}: owner read/write, subscription subdoc writeable only by Cloud Functions (not client).
      - match /pools/{poolId}: read/write if request.auth.uid == resource.data.ownerId || request.auth.uid in resource.data.members.
      - match /pools/{poolId}/logs/{logId}: create if authed user is owner or has contributor role in the pool's members map. Read if owner/contributor/viewer.
  Require request.auth.token.email_verified == true OR sign_in_provider == 'anonymous' (ISC-009, Path B). Require owner-has-active-premium via
  get(/databases/$(database)/documents/users/$(pool.data.ownerId)).data.subscription.status in ['active','trial','grace_period'] (ISC-005, inherited premium per L5-13-0).
      - match /{path=**}/logs/{logId}: collection group read rule mirroring the subcollection one (ISC-007).
      - match /chemicalStandards/{id}: system docs read-all (isSystem == true), custom docs owner-only.
    - firebase/firestore.indexes.json: composite index on logs collection group (poolId, testedAt desc) and (recordedBy, testedAt desc).
    - firebase/functions/test/rules.test.ts — rules unit tests against the emulator using @firebase/rules-unit-testing. Cover: owner writes, member writes,
  inherited-premium denial (owner lost entitlement), unverified email denial (non-anonymous), collection group query allowed. See firebase/rules-tests/ for the L5-13-0 anonymous-invitee behavioral suite.
  - Verification Step: cd firebase && firebase emulators:exec "npm test --prefix functions" green.
  - Human Check Needed?: No.

  ---
  Layer 3 — Security

  [Slice ID: L3-01] - Firebase Auth Repository

  - Objective: Wrap Firebase Auth behind a repository interface exposing a Flow<AuthState>.
  - Target LOC: ~400
  - Context to Update: project_state.md → "Security / Auth Repository"
  - Implementation Details:
    - domain/repository/AuthRepository.kt interface: observeAuthState(): Flow<AuthState>, signUpWithEmail(email, password), signInWithEmail(email, password),
  signInWithGoogle(idToken), sendVerificationEmail(), isEmailVerified(), reloadUser(), signOut(), deleteAccount().
    - Sealed AuthState: Unauthenticated, Authenticated(userId, emailVerified).
    - data/repository/AuthRepositoryImpl.kt: uses FirebaseAuth.getInstance(). Converts FirebaseAuthException to typed AuthError sealed class (invalid
  credentials, email in use, network, weak password, etc.).
    - Google Sign-In via Credential Manager API (modern replacement for deprecated GoogleSignInClient). SignInWithGoogleOption with server client ID from
  R.string.default_web_client_id.
    - Hilt AuthModule binds interface to impl.
    - Unit tests using MockK: sign-in success, wrong password error mapping, sign-out emits Unauthenticated.
  - Verification Step: ./gradlew :app:testDebugUnitTest --tests "*AuthRepositoryImplTest*" green.
  - Human Check Needed?: Yes — requires the SHA-1/SHA-256 fingerprints registered in the Firebase Console for Google Sign-In to work on real devices.

  [Slice ID: L3-02] - Token & Credential Storage

  - Objective: Encrypted local storage for any auth-related state the Firebase SDK doesn't own (e.g., last-known verified status cache for offline UX).
  - Target LOC: ~200
  - Context to Update: project_state.md → "Security / Credential Storage"
  - Implementation Details:
    - data/local/security/SecureStore.kt: wraps EncryptedSharedPreferences with a typed API (getBoolean, putString, etc.) — same MasterKey as L2-01 but a
  different file (aquaops_secure_prefs).
    - Stores: last_known_email_verified, last_known_user_id, billing_purchase_token (cache until server-verified — ISC-004).
    - SubscriptionEntity is authoritative once cloud-verified; SecureStore is the offline fallback only.
    - Unit test with Robolectric: write + read + encrypted-at-rest assertion (file contents are not plaintext).
  - Verification Step: ./gradlew :app:testDebugUnitTest --tests "*SecureStoreTest*" green.
  - Human Check Needed?: No.

  [Slice ID: L3-03] - Account Deletion & Data Wipe

  - Objective: Implement the PRD-mandated destructive account deletion flow (local wipe + cloud wipe).
  - Target LOC: ~300
  - Context to Update: project_state.md → "Security / Account Deletion"
  - Implementation Details:
    - DeleteAccountUseCase: (1) call a Cloud Function deleteUserAccount (slice L4-23) to wipe Firestore/Storage server-side; (2) sign out; (3) clear Room
  (database.clearAllTables()); (4) clear SecureStore; (5) remove the Keystore alias aquaops_db_key (next app launch regenerates it with an empty DB); (6) clear
  FCM token.
    - Firebase Auth FirebaseUser.delete() called last, after server cleanup confirmed.
    - UI-layer integration comes in L5 but the use case + repository method land here.
    - Instrumentation test against Firestore emulator: create user + pool + logs, invoke delete, verify all Firestore docs gone and Room is empty.
  - Verification Step: ./gradlew :app:connectedDebugAndroidTest --tests "*DeleteAccountTest*" green.
  - Human Check Needed?: No.

  ---
  Layer 4 — Logic (Domain + Repos + Cloud Functions)

  [Slice ID: L4-01] - Domain Value Objects & IDs

  - Objective: Typed primitives that prevent mixing pool/user/log/standard identifiers and carry units.
  - Target LOC: ~350
  - Context to Update: project_state.md → "Domain / Value Objects"
  - Implementation Details:
    - domain/model/ids.kt: @JvmInline value class PoolId(val value: UUID), UserId(String), LogEntryId(UUID), StandardId(UUID), ReagentTrackerId(UUID).
    - Volume(amount: Double, unit: VolumeUnit) with toGallons() / toLiters() conversions.
    - ChemicalReadings immutable data class with every parameter nullable. Computed totalChlorine: Double? (ISC-011).
    - ParameterRange(min: Double?, max: Double?, yellowMargin: Double = 0.10).
    - Dosage(amount: Double, unit: MassUnit, chemical: ChemicalType).
    - Status enum (GREEN, YELLOW, RED); ParameterStatus(parameter, value, status, range).
    - Parameter enum listing all measurable properties.
    - Unit tests for every computed property: totalChlorine null when either input null, volume conversions, yellowMargin defaulting.
  - Verification Step: ./gradlew :app:testDebugUnitTest --tests "com.chkdsklabs.aquaops.domain.model.*" green.
  - Human Check Needed?: No.

  [Slice ID: L4-02] - Domain Aggregates & Entity Mappers

  - Objective: Pure-Kotlin domain aggregates + bidirectional mappers to Room entities.
  - Target LOC: ~400
  - Context to Update: project_state.md → "Domain / Aggregates"
  - Implementation Details:
    - domain/model/Pool.kt, LogEntry.kt, ChemicalStandard.kt, PoolMembership.kt, Subscription.kt, ReagentTracker.kt, AlertConfig.kt. All immutable, no Android
  imports.
    - EntitlementState enum: NONE, TRIAL, ACTIVE, GRACE_PERIOD, EXPIRED.
    - MembershipState: PENDING, ACCEPTED, SUSPENDED, REMOVED; MembershipRole: OWNER, CONTRIBUTOR, VIEWER.
    - data/local/mapper/DomainMappers.kt: PoolEntity.toDomain(), Pool.toEntity(), etc.
    - Unit tests for each mapper roundtrip.
  - Verification Step: ./gradlew :app:testDebugUnitTest --tests "*DomainMappersTest*" green.
  - Human Check Needed?: No.

  [Slice ID: L4-03] - Repository Interfaces

  - Objective: Domain-layer repository contracts that UI/use cases depend on.
  - Target LOC: ~200
  - Context to Update: project_state.md → "Domain / Repositories"
  - Implementation Details:
    - domain/repository/: PoolRepository, LogEntryRepository, ChemicalStandardRepository, PoolMembershipRepository, SubscriptionRepository,
  ReagentTrackerRepository, AlertConfigRepository.
    - Every read method returns Flow<...> backed by Room (ISC-001 — never Firestore for UI).
    - Every write method is suspend and returns Result<Unit> or a typed outcome.
    - Repositories accept domain models, never DTOs/entities.
  - Verification Step: ./gradlew :app:compileDebugKotlin passes.
  - Human Check Needed?: No.

  [Slice ID: L4-04] - PoolRepository Implementation

  - Objective: Room-first, Firestore-synced Pool repository.
  - Target LOC: ~350
  - Context to Update: project_state.md → "Repos / PoolRepository"
  - Implementation Details:
    - PoolRepositoryImpl(dao, firestoreSource, userId, ioDispatcher):
      - observePools() → Flow from DAO, mapped to domain.
      - save(pool) → (1) upsert Room with syncedAt = null; (2) launch coroutine to write DTO to Firestore with FieldValue.serverTimestamp(); (3) on Firestore
  success, update Room syncedAt.
      - delete(poolId) → delete from Room and Firestore (cascade subcollections triggered by Cloud Function).
      - Enforce pool-count rule via injected EntitlementGateService before save.
    - Attach Firestore snapshot listener when user is authenticated + email-verified; merges remote changes into Room with server timestamp preserved (ISC-006).
    - Integration test with Room in-memory + Firestore emulator.
  - Verification Step: ./gradlew :app:connectedDebugAndroidTest --tests "*PoolRepositoryImplTest*" green against the emulator.
  - Human Check Needed?: No.

  [Slice ID: L4-05] - LogEntryRepository Implementation

  - Objective: Log entry CRUD with synced_at tracking and event emission on save.
  - Target LOC: ~450
  - Context to Update: project_state.md → "Repos / LogEntryRepository"
  - Implementation Details:
    - Same Room-first pattern. Subcollection path pools/{poolId}/logs/{logId} in Firestore.
    - Critical: synced_at only set by the snapshot listener callback, never by the write path (ISC-002).
    - On saveLogEntry() success, publish LogEntrySaved event on the injected DomainEventBus (L4-17).
    - Pull sync: snapshot listener handles DocumentChange.Type.REMOVED by checking deletedByRetention flag — if true, skip local delete (ISC-008); if false
  (user-initiated), delete from Room.
    - Filter/search methods delegate to DAO.
    - Integration tests covering: offline save → online sync confirms → syncedAt populated; retention cleanup doesn't cascade to Room; contributor write from
  another device appears via snapshot listener.
  - Verification Step: ./gradlew :app:connectedDebugAndroidTest --tests "*LogEntryRepositoryImplTest*" green.
  - Human Check Needed?: No.

  [Slice ID: L4-06] - Standard, Membership, Reagent, AlertConfig Repositories

  - Objective: Implementations for the remaining simple aggregates.
  - Target LOC: ~450
  - Context to Update: project_state.md → "Repos / Misc"
  - Implementation Details:
    - ChemicalStandardRepositoryImpl: guards against modification of isSystem = true standards (ISC throws SystemStandardImmutableException).
    - PoolMembershipRepositoryImpl: invitation ops delegate to Cloud Function (L4-23; replaced by L5-13a acceptInvitation which checks inviter entitlement per Path B); local mirror in Room updated via snapshot
  listener.
    - ReagentTrackerRepositoryImpl: plain Room-first + Firestore sync.
    - AlertConfigRepositoryImpl: same.
    - Integration tests for each against emulator.
  - Verification Step: ./gradlew :app:connectedDebugAndroidTest --tests "*RepositoryImplTest*" green.
  - Human Check Needed?: No.

  [Slice ID: L4-07] - SubscriptionRepository + BillingClient Wrapper

  - Objective: Wrap Google Play Billing and Firestore-synced subscription state.
  - Target LOC: ~450
  - Context to Update: project_state.md → "Repos / SubscriptionRepository" (document Billing product IDs)
  - Implementation Details:
    - data/remote/billing/BillingClientWrapper.kt: Billing Library v7+. Lifecycle-aware connection. Methods: queryProducts(), launchPurchaseFlow(activity,
  productDetails), acknowledgePurchase(token), queryPurchases() (both SUBS).
    - SubscriptionRepositoryImpl:
      - observeEntitlement() → Flow from SubscriptionDao (Firestore-synced, ISC-004).
      - Fallback: if Firestore doc stale > 6h and BillingClient shows active purchase, display "verifying…" state but don't unlock features without server
  confirmation.
      - On BillingClient purchase-updated callback: call Cloud Function verifyPurchase (L4-24) with the purchase token → function writes the verified
  subscription doc → snapshot listener propagates to Room.
    - Integration test: mock BillingClient emits a purchase → verify Cloud Function called → Firestore doc populated → Room synced.
  - Verification Step: ./gradlew :app:connectedDebugAndroidTest --tests "*SubscriptionRepositoryImplTest*" green.
  - Human Check Needed?: Yes — requires Play Console product IDs (aquaops_premium_monthly, aquaops_premium_annual) configured.

  [Slice ID: L4-08] - Domain Event Bus

  - Objective: In-process event bus for cross-context coordination.
  - Target LOC: ~150
  - Context to Update: project_state.md → "Domain / Event Bus"
  - Implementation Details:
    - domain/event/DomainEvent.kt sealed class: LogEntrySaved, LogEntryEdited, PoolCreated, PoolDeleted, SubscriptionStateChanged, MembershipAccepted,
  MembershipRevoked, TrendDetected, ReagentLow.
    - DomainEventBus: singleton wrapping a MutableSharedFlow<DomainEvent> with replay=0, extraBufferCapacity=64.
    - Unit test: emit + collect roundtrip.
  - Verification Step: ./gradlew :app:testDebugUnitTest --tests "*DomainEventBusTest*" green.
  - Human Check Needed?: No.

  [Slice ID: L4-09] - ChemicalComparisonService

  - Objective: Compare readings against a standard, producing per-parameter status + worst-status.
  - Target LOC: ~350
  - Context to Update: project_state.md → "Domain / ChemicalComparisonService"
  - Implementation Details:
    - Pure function. Input: ChemicalReadings, ChemicalStandard. Output: Map<Parameter, ParameterStatus> + worstStatus(): Status.
    - Logic: for each non-null reading, look up the ParameterRange. GREEN if in range. YELLOW if within yellowMargin of either boundary. RED if outside. Skip
  null readings. Skip totalChlorine when either input is null (ISC-011).
    - Extensive unit tests: boundary cases, yellow margin math, null reading skipping, custom margin values.
  - Verification Step: ./gradlew :app:testDebugUnitTest --tests "*ChemicalComparisonServiceTest*" green, 90%+ coverage.
  - Human Check Needed?: No.

  [Slice ID: L4-10] - DosageCalculationService

  - Objective: Calculator formulas for every chemical in PRD 4.2.4.
  - Target LOC: ~500
  - Context to Update: project_state.md → "Domain / DosageCalculationService" (list every supported chemical + formula constants)
  - Implementation Details:
    - One function per chemical: liquid chlorine (12.5%), granular cal-hypo (65%), trichlor tabs (90%), muriatic acid (31%), sodium carbonate, sodium bisulfate,
   sodium bicarbonate for alkalinity, muriatic acid for alk, calcium chloride, cyanuric acid, salt (target ppm), shock breakpoint chlorination.
    - Every formula uses gallons internally; auto-converts from liters at the boundary.
    - Output: Dosage(amount, unit, chemical) with sensible unit picked (oz, lb, g, kg) based on magnitude.
    - Unit tests with known-good dosage tables (reference: Taylor/LaMotte common pool chemistry tables). Precision: ±5% of industry calculator output.
  - Verification Step: ./gradlew :app:testDebugUnitTest --tests "*DosageCalculationServiceTest*" green, 95%+ coverage.
  - Human Check Needed?: Yes — a chemistry SME should spot-check the reference table values before release. Incorrect dosage = real-world safety issue.

  [Slice ID: L4-11] - TrendAnalysisService + EntitlementGateService

  - Objective: Trend detection and the central "can this user do this?" gate.
  - Target LOC: ~300
  - Context to Update: project_state.md → "Domain / Services"
  - Implementation Details:
    - TrendAnalysisService.detectTrends(lastN: List<LogEntry>, standard): List<Trend>. Requires n≥3; returns direction + magnitude toward nearest red boundary.
  Used by Cloud Function trend evaluator and optional in-app chart.
    - EntitlementGateService.canX(...): checks SubscriptionState, emailVerified, and feature-specific rules (pool count for free, inherited-premium inheritance for shared
  access per L5-13-0). Used by every premium-gated use case and ViewModel.
    - Unit tests for every gate: free tier owns 2 pools (blocked), premium in grace period (allowed), expired tier reading own pool (allowed, read-only), etc.
  - Verification Step: ./gradlew :app:testDebugUnitTest --tests "*{Trend,Entitlement}*" green.
  - Human Check Needed?: No.

  [Slice ID: L4-12] - Use Cases: Log Entry + Pool + Standard CRUD

  - Objective: Thin orchestration layer between ViewModels and repositories, with validation and entitlement checks.
  - Target LOC: ~500
  - Context to Update: project_state.md → "Domain / Use Cases"
  - Implementation Details:
    - SaveLogEntryUseCase(repo, comparisonService, entitlementGate, eventBus): validates ranges (pH 0–14, etc.), checks write permission (owner/contributor),
  persists, emits LogEntrySaved.
    - EditLogEntryUseCase, DeleteLogEntryUseCase.
    - CreatePoolUseCase, UpdatePoolUseCase, DeletePoolUseCase (cascade local data).
    - CreateStandardUseCase (premium-only), UpdateStandardUseCase, DeleteStandardUseCase.
    - CompareLogEntryUseCase, CalculateDosageUseCase (thin wrappers around domain services).
    - Unit tests for each use case with MockK.
  - Verification Step: ./gradlew :app:testDebugUnitTest --tests "com.chkdsklabs.aquaops.domain.usecase.*" green, 90%+ coverage.
  - Human Check Needed?: No.

  [Slice ID: L4-13] - Sync Manager + WorkManager Reconciliation

  - Objective: Orchestrate Firestore snapshot listeners (foreground) + periodic WorkManager job (safety net).
  - Target LOC: ~400
  - Context to Update: project_state.md → "Sync / SyncManager" (document 6h periodic cadence)
  - Implementation Details:
    - SyncManager singleton, lifecycle-aware via ProcessLifecycleOwner. On foreground + authenticated + verified + premium, register snapshot listeners for
  pools, pools/*/logs (collection group), chemicalStandards (owner-scoped), etc. Unregister on background.
    - ReconciliationWorker extends CoroutineWorker: queries Room for syncedAt IS NULL, pushes to Firestore; queries Firestore for updatedAt >
  local.max(updatedAt), merges into Room. Periodic every 6h.
    - SyncStatus sealed class emitted via Flow for UI banners (L5-19).
    - Integration tests: offline writes accumulate in Room; bring online → snapshot listener drains queue; force a 6h reconciliation → verify divergence
  resolved.
  - Verification Step: ./gradlew :app:connectedDebugAndroidTest --tests "*SyncManagerTest*" green.
  - Human Check Needed?: No.

  [Slice ID: L4-14] - Cloud Functions Scaffold (TypeScript)

  - Objective: Initialize the Functions codebase so subsequent slices can deploy triggers.
  - Target LOC: ~250
  - Context to Update: project_state.md → "Cloud Functions / Scaffold"
  - Implementation Details:
    - firebase/functions/ — TypeScript, Node 20, ESLint, Jest. package.json scripts: lint, test, build, deploy.
    - Modular structure: src/billing/, src/retention/, src/alerts/, src/invitations/, src/userDelete/.
    - Shared src/firebase.ts initializing admin SDK.
    - Shared src/types.ts mirroring Firestore document shapes (kept in sync with Kotlin DTOs).
    - src/index.ts exports all functions.
    - One no-op healthCheck HTTPS function to validate deploy.
  - Verification Step: cd firebase/functions && npm ci && npm run build && npm test; firebase deploy --only functions:healthCheck --project=aquaops-dev then
  curl https://.../healthCheck returns 200.
  - Human Check Needed?: Yes — first deploy requires Firebase Blaze plan + billing enabled on the dev project.

  [Slice ID: L4-15] - Cloud Function: Subscription Verification

  - Objective: Server-authoritative subscription verification (ISC-004).
  - Target LOC: ~400
  - Context to Update: project_state.md → "Cloud Functions / verifyPurchase"
  - Implementation Details:
    - HTTPS callable verifyPurchase(purchaseToken, productId).
    - Uses googleapis to call androidpublisher.purchases.subscriptionsv2.get.
    - Maps Google Play state → EntitlementState: SUBSCRIPTION_STATE_ACTIVE → ACTIVE, IN_GRACE_PERIOD → GRACE_PERIOD, ON_HOLD/CANCELED/EXPIRED → EXPIRED, trial
  detection via autoRenewingPlan.trialStartTime.
    - Writes /users/{uid}/subscription with state, plan, currentPeriodStart, currentPeriodEnd, graceDeadline, verifiedAt.
    - Realtime Developer Notifications (RTDN) trigger via Pub/Sub playBillingNotifications topic — also in this slice — updates the same doc on renewal/cancel.
    - Service account with Play Developer API access stored in Firebase secret manager (PLAY_API_SA_KEY).
    - Jest tests mocking the Play API: happy path, grace period, hold, canceled, invalid token.
  - Verification Step: cd firebase/functions && npm test -- subscription.test.ts; manual emulator call returns correct subscription doc shape.
  - Human Check Needed?: Yes — requires Play Console API access + service account + RTDN Pub/Sub topic setup.

  [Slice ID: L4-16] - Cloud Function: 90-Day Retention (Soft Delete)

  - Objective: Retention cleanup that doesn't cascade to local (ISC-008).
  - Target LOC: ~200
  - Context to Update: project_state.md → "Cloud Functions / retentionCleanup"
  - Implementation Details:
    - Scheduled via pubsub.schedule('every day 04:00') (UTC).
    - Collection group query on logs where createdAt < now - 90 days.
    - Soft delete: set deletedByRetention = true on the doc, then delete after a 24h grace (two-phase). Clients see the flag in the snapshot event and know not
  to cascade to Room.
    - Batch in chunks of 400 (Firestore batch limit 500 with buffer).
    - Jest test with Firestore emulator: seed 100 docs spanning dates, run function, assert only old docs flagged.
  - Verification Step: npm test -- retention.test.ts; emulator integration pass.
  - Human Check Needed?: No.

  [Slice ID: L4-17] - Cloud Function: Out-of-Balance + Trend Alerts

  - Objective: Evaluate log writes and send FCM pushes.
  - Target LOC: ~450
  - Context to Update: project_state.md → "Cloud Functions / onLogCreate"
  - Implementation Details:
    - Firestore trigger onDocumentCreated("pools/{poolId}/logs/{logId}").
    - Load pool's active standard (system or owner's custom). Run same comparison logic as client ChemicalComparisonService (port a TS version; share constants
  via JSON if practical).
    - If worst status is RED → out-of-balance push to pool owner + all contributors with active subscriptions.
    - Load last 3 entries for this pool+parameter; run trend detection; if trend → trend push.
    - Respect per-pool AlertConfig + quiet hours (defer delivery via Cloud Tasks if inside quiet window).
    - Skip if user's subscription isn't in {TRIAL, ACTIVE, GRACE_PERIOD}.
    - Jest tests for each alert condition.
  - Verification Step: npm test -- alerts.test.ts; emulator integration: write a red log → FCM mock receives message.
  - Human Check Needed?: No.

  [Slice ID: L4-18] - Cloud Function: Test Reminders + Reagent Low

  - Objective: Scheduled reminder fan-out + reagent-low event handler.
  - Target LOC: ~300
  - Context to Update: project_state.md → "Cloud Functions / reminders"
  - Implementation Details:
    - Scheduled every hour. For each AlertConfig where testReminderEnabled, check last log timestamp; if age > testReminderIntervalHours, send FCM.
    - Firestore trigger onDocumentWritten("reagentTrackers/{id}"): if remainingTests <= alertThreshold and prior value was above, send FCM.
    - Respect quiet hours.
    - Jest tests.
  - Verification Step: npm test -- reminders.test.ts green.
  - Human Check Needed?: No.

  [Slice ID: L4-19] - Cloud Function: Invitation Accept + Account Delete

  - Objective: Server-enforced inviter entitlement check on invite-accept (ISC-005 inherited premium, Path B) + destructive account wipe. (Original L4-19 shipped bilateral checks; superseded by L5-13-0 rewrite + L5-13a Cloud Function replacement.)
  - Target LOC: ~400
  - Context to Update: project_state.md → "Cloud Functions / invitations, userDelete"
  - Implementation Details:
    - HTTPS callable acceptInvitation(poolId): verify invitee's subscription is active, verify pool owner's subscription is active, flip membership state to
  ACCEPTED, emit event.
    - HTTPS callable deleteUserAccount(): recursive delete of /users/{uid}, all /pools/* where ownerId == uid, membership entries referencing uid, reagent
  trackers, alert configs. Returns when complete.
    - On invitee subscription lapse (Pub/Sub RTDN CANCELED/EXPIRED) → auto-suspend that user's memberships.
    - Jest tests for each function including the bilateral denial case.
  - Verification Step: npm test -- invitations.test.ts userDelete.test.ts green.
  - Human Check Needed?: No.

  ---
  Layer 5 — Interface

  [Slice ID: L5-01] - Shared Compose Components

  - Objective: Reusable primitives embodying the design style (buttons, cards, inputs, status indicators).
  - Target LOC: ~500
  - Context to Update: project_state.md → "UI / Components"
  - Implementation Details:
    - ui/components/AquaButton.kt: Primary, Secondary, Tertiary, Destructive, FAB variants per §5.1. 48dp min touch target.
    - AquaCard.kt: outlined, flat, 12dp radius, optional 3dp left status bar.
    - AquaTextField.kt: outlined 8dp radius with status border on error; numeric variant with Body Large 500 weight.
    - StatusDot, StatusBadge, StatusBar, StatusRowBackground components per §5.4 — always paired with a text label/icon.
    - SyncIndicator: pulse animation per §7.
    - Preview composables for light + dark.
    - Compose UI tests: createComposeRule() asserts status badge shows the expected text label and content description.
  - Verification Step: ./gradlew :app:connectedDebugAndroidTest --tests "com.chkdsklabs.aquaops.ui.components.*" green.
  - Human Check Needed?: No.

  [Slice ID: L5-02] - Navigation Graph

  - Objective: Single-activity nav graph with auth + main graphs.
  - Target LOC: ~300
  - Context to Update: project_state.md → "UI / Navigation"
  - Implementation Details:
    - ui/navigation/AquaOpsNavHost.kt. Top-level routes: auth/*, main/*, subscription/*.
    - Nested: auth/signIn, auth/signUp, auth/verifyEmail. main/dashboard, main/log/new, main/log/history, main/log/{id}, main/pools, main/pools/{id},
  main/calculators, main/settings, main/standards, main/shared, main/export, main/reports, main/alerts, main/reagents.
    - AuthGate composable observes AuthState + redirects appropriately.
    - Screen transitions use shared-axis horizontal per §7. Respect reduce-motion.
    - UI test: nav controller starts at auth when unauthenticated, main when authenticated.
  - Verification Step: Compose UI test *NavigationTest* green.
  - Human Check Needed?: No.

  [Slice ID: L5-03] [Phase 1] - Auth Screens

  - Objective: Sign up, sign in (email + Google), email verification prompt.
  - Target LOC: ~500
  - Context to Update: project_state.md → "UI / Auth"
  - Implementation Details:
    - SignUpScreen, SignInScreen, VerifyEmailScreen with corresponding ViewModels (@HiltViewModel).
    - Validation inline (8-char min, 1 letter + 1 number per PRD §6).
    - Google Sign-In button invokes Credential Manager.
    - VerifyEmailScreen: periodic reloadUser() tick every 5s; on verified, nav to dashboard.
    - Compose UI tests for the happy path of each screen.
  - Verification Step: ./gradlew :app:connectedDebugAndroidTest --tests "*{SignUp,SignIn,Verify}*" green.
  - Human Check Needed?: No.

  [Slice ID: L5-04] [Phase 1] - Dashboard Screen

  - Objective: At-a-glance pool status with every parameter visible above the fold (design §4.3).
  - Target LOC: ~450
  - Context to Update: project_state.md → "UI / Dashboard"
  - Implementation Details:
    - DashboardViewModel: combines flows from PoolRepository.observePools() and LogEntryRepository.observeMostRecent(poolId) + ChemicalComparisonService.
    - DashboardScreen: pool name (Headline), last-test timestamp, "days since last test" with nudge badge if > 3, a 2-col parameter grid (3-col on tablet)
  showing name + value + status dot + status badge, FAB for new log.
    - Premium: pool switcher chip in top app bar.
    - Empty state for no logs yet.
    - Compose UI test: renders correct status for a seeded log, FAB navigates to new log screen.
  - Verification Step: ./gradlew :app:connectedDebugAndroidTest --tests "*DashboardTest*" green.
  - Human Check Needed?: No.

  [Slice ID: L5-05] [Phase 1] - Log Entry Form Screen

  - Objective: The primary "create a log entry" form with full validation and auto-calc preview.
  - Target LOC: ~500
  - Context to Update: project_state.md → "UI / LogEntryForm"
  - Implementation Details:
    - LogEntryFormViewModel: holds LogEntryFormState, validates each field on change, debounces comparison preview (200ms).
    - LogEntryFormScreen: numeric inputs per parameter (Body Large 500), unit toggle (F/C) for temp, notes field (500 char counter), live preview card showing
  per-parameter status as the user types.
    - Save button disabled until at least one reading is present. On save: dispatches SaveLogEntryUseCase, navigates back with a success snackbar ("Log saved.
  Sync pending." if offline, "Log saved." if online).
    - Compose UI tests: invalid pH shows error, save disabled when empty, save success navigates back.
  - Verification Step: *LogEntryFormTest* green.
  - Human Check Needed?: No.

  [Slice ID: L5-06] [Phase 1] - Log History Screen

  - Objective: Scrollable, filterable, searchable list of past log entries.
  - Target LOC: ~400
  - Context to Update: project_state.md → "UI / LogHistory"
  - Implementation Details:
    - LogHistoryViewModel: Flow from DAO observeByPoolAndDateRange + search query state.
    - LogHistoryScreen: date-range picker, search field for notes, sticky month headers, list items showing date/time + worst status bar/dot + one-line
  parameter summary per design §4.3. Tap to expand to full detail card (vertical expand + fade 250ms).
    - Pagination via Paging 3 if list exceeds 200 items.
    - Compose UI test: filter narrows list, search narrows list, expand reveals full detail.
  - Verification Step: *LogHistoryTest* green.
  - Human Check Needed?: No.

  [Slice ID: L5-07] [Phase 1] - Pool Profile Screen

  - Objective: Create / edit / delete a pool profile.
  - Target LOC: ~350
  - Context to Update: project_state.md → "UI / PoolProfile"
  - Implementation Details:
    - PoolProfileViewModel (create + edit modes).
    - PoolProfileScreen: name, volume w/ unit toggle, pool type, surface type, sanitizer dropdowns. Destructive delete button with confirmation dialog (scrim
  overlay per §5.6).
    - Free-tier users blocked from creating a 2nd pool with a Paywall sheet.
    - Compose UI tests.
  - Verification Step: *PoolProfileTest* green.
  - Human Check Needed?: No.

  [Slice ID: L5-08] [Phase 1] - Settings + Account Screen

  - Objective: Theme, sign-out, account deletion, about.
  - Target LOC: ~300
  - Context to Update: project_state.md → "UI / Settings"
  - Implementation Details:
    - Sections: Account (email, verified badge, sign-out, delete account with 2-step confirm), Appearance (follow-system / light / dark toggle), Data (manual
  sync trigger, export — greyed out in free tier), About (version, links).
    - Delete-account confirmation is a two-step dialog (type "DELETE" to confirm) before invoking DeleteAccountUseCase.
  - Verification Step: *SettingsTest* green.
  - Human Check Needed?: No.

  [Slice ID: L5-09] - Subscription / Paywall Screens

  - Objective: Product selection + purchase flow entry point.
  - Target LOC: ~400
  - Context to Update: project_state.md → "UI / Subscription"
  - Implementation Details:
    - SubscriptionViewModel: observes entitlement + queryProducts() from BillingClient wrapper.
    - PaywallScreen: features list, Monthly / Annual cards with price + trial copy, primary CTA Start 14-day trial. On tap → launchPurchaseFlow(activity,
  productDetails).
    - ManageSubscriptionScreen: current plan, renewal date, link to Google Play manage-subscription deep link.
    - Grace-period banner with "Update payment" action (deep link).
    - Compose UI test with fake BillingClient.
  - Verification Step: *SubscriptionTest* green.
  - Human Check Needed?: No.

  [Slice ID: L5-10] - Standards Management Screen (Premium)

  - Objective: Create / edit custom chemical standards.
  - Target LOC: ~400
  - Context to Update: project_state.md → "UI / Standards"
  - Implementation Details:
    - StandardsListScreen, StandardEditScreen. Per-parameter min/max/yellowMargin editors.
    - MAHC 2 shown read-only with a "Clone to customize" action.
    - Active-standard selector on each pool (wire into Pool profile screen).
  - Verification Step: *StandardsTest* green.
  - Human Check Needed?: No.

  [Slice ID: L5-11] - Multi-Pool + Aggregate View (Premium)

  - Objective: Pool switcher, multi-pool dashboard, all-pools aggregate status.
  - Target LOC: ~400
  - Context to Update: project_state.md → "UI / MultiPool"
  - Implementation Details:
    - Top-app-bar pool dropdown (pill chip + caret per §5.5).
    - AggregateDashboardScreen: all-pools grid, each card shows pool name + worst-status from most-recent log. Uses Firestore collection group query via Room's
  already-synced data (ISC-007, queries local Room).
    - Free-tier redirect to paywall when tapping the pool switcher with > 1 pool available (shouldn't happen, but defensive).
  - Verification Step: *AggregateDashboardTest* green.
  - Human Check Needed?: No.

  [Slice ID: L5-12] - Calculator Screens (Premium)

  - Objective: UI for every dosage calculator from L4-10.
  - Target LOC: ~500
  - Context to Update: project_state.md → "UI / Calculators"
  - Implementation Details:
    - CalculatorHubScreen: grid of calculators. Each CalculatorScreen variant (ChlorineCalculator, PhCalculator, AlkalinityCalculator, CalciumCalculator,
  CyanuricCalculator, SaltCalculator, ShockCalculator).
    - Auto-fills current reading from most recent log, target from active standard (per PRD 4.2.4).
    - Output card shows dosage amount + chemical name + unit toggle (oz/lb/g/kg). Safety note banner.
    - Compose UI tests with known-input/known-output assertions from the test fixture from L4-10.
  - Verification Step: *CalculatorTest* green.
  - Human Check Needed?: Yes — visual design review of dosage safety messaging.

  [Slice ID: L5-13] - Shared Pool Screens (Premium)

  - Objective: Invite, accept, manage members.
  - Target LOC: ~450
  - Context to Update: project_state.md → "UI / Sharing"
  - Implementation Details:
    - MembersListScreen (owner): pending/accepted members, role changer, remove action.
    - InviteMemberScreen: email input + role picker. Generates a share link via Firebase Dynamic Links (or App Links successor — confirm infra).
    - AcceptInvitationScreen: deep-link handler, calls acceptInvitation Cloud Function; shows error if invitee not premium (per ISC-005).
  - Verification Step: *SharingTest* green.
  - Human Check Needed?: Resolved 2026-04-21 — delivery is Android App Links + Resend email via Cloud Function (see docs/project_state.md "Invitation delivery mechanism"). Dynamic Links (deprecated) is not used.

  [Slice ID: L5-14] - Export + PDF Report Screens (Premium)

  - Objective: CSV/XLSX export and formatted PDF compliance reports.
  - Target LOC: ~500
  - Context to Update: project_state.md → "UI / Export, Reports"
  - Implementation Details:
    - ExportScreen: pool picker (multi-select), date range, parameter checkboxes, notes toggle, format picker (CSV/XLSX). On generate: writes to Downloads/ via
  MediaStore, fires ACTION_SEND share intent.
    - XLSX via Apache POI-ooxml (note APK size cost — confirm budget).
    - ReportScreen: same selectors + template preview. PDF generation via Android's PdfDocument or iText (license-permissive variant). Includes: pool metadata,
  date range, log table with status colors, min/max/avg per parameter, line charts (via Canvas compose or MPAndroidChart), compliance %, standard used.
    - Target: 90 days of daily logs render in < 10s (PRD 8.1).
    - Optional: upload to Cloud Storage and share a signed URL.
    - Compose UI test + integration test for file generation correctness.
  - Verification Step: *{Export,Report}Test* green; manual render sanity check.
  - Human Check Needed?: Yes — Apache POI adds ~5MB to APK. Confirm whether that size increase is acceptable vs writing a minimal XLSX-from-scratch
  implementation.

  [Slice ID: L5-15] - Alert Config + Reagent Tracker Screens (Premium)

  - Objective: UI for per-pool alert configuration + reagent inventory.
  - Target LOC: ~400
  - Context to Update: project_state.md → "UI / Alerts, Reagents"
  - Implementation Details:
    - AlertConfigScreen: per-alert-type toggles (out-of-balance, test reminder, trend), quiet-hours time picker, reminder interval slider.
    - ReagentListScreen + ReagentEditScreen: reagent name, total tests, remaining (auto-decrementing display), alert threshold, supplier URL with "Open" action.
  - Verification Step: *{Alert,Reagent}ScreenTest* green.
  - Human Check Needed?: No.

  ---
  Layer 6 — Features (Cross-Cutting)

  [Slice ID: L6-01] - FCM Token Registration + Push Handling

  - Objective: Receive pushes and route to the right in-app destination.
  - Target LOC: ~300
  - Context to Update: project_state.md → "Features / FCM"
  - Implementation Details:
    - AquaOpsMessagingService : FirebaseMessagingService. On onNewToken, write to /users/{uid}/fcmTokens/{deviceId} (gated by email verification — ISC-009).
    - onMessageReceived: parse alertType (out-of-balance, test-reminder, trend, reagent-low); build notification with status-colored accent; deep-link intent to
   the relevant pool's dashboard or reagent list.
    - Notification channels per alert type for user control.
    - Integration test via Firebase test lab or emulator.
  - Verification Step: ./gradlew :app:testDebugUnitTest --tests "*MessagingServiceTest*" green; manual smoke test firing a test message from Firebase Console.
  - Human Check Needed?: No.

  [Slice ID: L6-02] - Sync Status Banner + Manual Sync Trigger

  - Objective: Always-visible sync indicator per design §5.5 + manual sync in settings.
  - Target LOC: ~200
  - Context to Update: project_state.md → "Features / SyncUI"
  - Implementation Details:
    - SyncBanner composable subscribed to SyncManager.syncStatus flow. States: Synced (hidden), Pending(n) (info banner), Offline (warning banner), Error(msg)
  (brick tint).
    - Manual sync button in Settings calls SyncManager.forceReconciliation().
    - Compose UI test per state.
  - Verification Step: *SyncBannerTest* green.
  - Human Check Needed?: No.

  [Slice ID: L6-03] - Analytics Event Wiring

  - Objective: Emit the PRD §9.2 events via Firebase Analytics.
  - Target LOC: ~250
  - Context to Update: project_state.md → "Features / Analytics"
  - Implementation Details:
    - AnalyticsService wrapping FirebaseAnalytics with typed event methods: logEntryCreated, calculatorUsed, exportGenerated, pdfGenerated, poolCreated,
  poolDeleted, inviteSent, inviteAccepted, subscriptionStarted, subscriptionCancelled, subscriptionRenewed, alertTriggered, alertTapped.
    - Call sites from ViewModels/use cases.
    - Opt-out toggle in Settings.
  - Verification Step: Unit tests verify each event fires with correct params via a fake analytics sink.
  - Human Check Needed?: No.

  [Slice ID: L6-04] - Tablet-Adaptive Layout

  - Objective: Two-pane dashboard + history on tablets per PRD 8.5.
  - Target LOC: ~350
  - Context to Update: project_state.md → "Features / Adaptive"
  - Implementation Details:
    - WindowSizeClass detection; 3-column param grid on expanded width (§4.2).
    - Two-pane: dashboard on left, log history on right (ListDetailPaneScaffold from Compose adaptive).
    - UI test at expanded window class.
  - Verification Step: *AdaptiveLayoutTest* green on tablet emulator AVD.
  - Human Check Needed?: No.

  [Slice ID: L6-05] - Home Screen Widget

  - Objective: Glanceable last-test-status widget.
  - Target LOC: ~350
  - Context to Update: project_state.md → "Features / Widget"
  - Implementation Details:
    - Built with Jetpack Glance (Compose-for-widgets).
    - Widget content: pool name, worst-status dot, days-since-last-test, tap-through deep link to dashboard.
    - Updates on log save via WorkManager unique worker.
  - Verification Step: Manual install on emulator home screen + update on log save.
  - Human Check Needed?: No.

  [Slice ID: L6-06] - Trend Chart Views (In-App)

  - Objective: Per-parameter line charts over time on the log detail / pool detail screen.
  - Target LOC: ~300
  - Context to Update: project_state.md → "Features / Charts"
  - Implementation Details:
    - Uses Canvas Compose (no third-party dep). Vertical axis: parameter value; horizontal: time. Plot reference range as a tinted band.
    - Interactive: tap a point shows the exact reading + date.
    - Respect reduce-motion (no animated draw-on).
  - Verification Step: Compose UI test renders + screenshot diff.
  - Human Check Needed?: No.

  [Slice ID: L6-07] - Final Acceptance + Release Prep

  - Objective: Cross-cutting acceptance pass before Phase 1 ships.
  - Target LOC: ~200 (mostly config + docs)
  - Context to Update: project_state.md → "Release / Phase 1 Closure"
  - Implementation Details:
    - app/proguard-rules.pro with Hilt/Room/Firestore/kotlinx.serialization keep rules.
    - R8 minify + resource shrink on release build.
    - App icons (adaptive) + splash (boho palette per §2.1).
    - Privacy policy URL + data safety form draft (Play Console).
    - Accessibility scan via Accessibility Scanner — address all critical findings.
    - Baseline profile generation to improve cold-start (< 2s target per PRD 8.1).
    - Smoke-test checklist: sign up → verify email → create pool → log 3 entries (online + offline) → verify sync → delete account.
  - Verification Step: ./gradlew :app:bundleRelease produces a signed AAB + smoke-test checklist all green on a physical device.
  - Human Check Needed?: Yes — Play Console submission readiness and data-safety disclosures need a human.

  [Slice ID: L6-08] - AdMob Integration for Free Tier (ADR-011)

  - Objective: Integrate Google AdMob as the ad network for free-tier users, with ads fully suppressed for ACTIVE / TRIAL / GRACE_PERIOD entitlement states. Enforces ISC-015 (entitlement transition removes ads within one frame) and the placement policy from ADR-011 §6.
  - Target LOC: ~500
  - Context to Update: docs/project_state.md → "Features / AdMob" (new section — SDK init, consent flow, placement whitelist, entitlement wiring, build-variant ad-unit-ID separation)
  - Implementation Details:
    - Add `com.google.android.gms:play-services-ads` and `com.google.android.ump:user-messaging-platform` to `gradle/libs.versions.toml`; wire into `app/build.gradle.kts`.
    - `data/remote/ads/ConsentManager.kt`: wraps UMP SDK. On cold start (and on Play-services consent-change broadcasts), calls `UserMessagingPlatform.getConsentInformation(...)` + `loadAndShowConsentFormIfRequired(...)`. Persists the IAB TCF v2 consent string via UMP's own storage.
    - `data/remote/ads/AdMobInitializer.kt`: `MobileAds.initialize(...)` invoked on a background thread after first frame (`Handler(Looper.getMainLooper()).post { ... }` from `AquaOpsApp.onCreate`, wrapped in `Dispatchers.IO.launch`). Must not block cold-start budget (PRD §8.1). Sets `setTagForChildDirectedTreatment(FALSE)` and `setTagForUnderAgeOfConsent(FALSE)` per ADR-011 §6.
    - `EntitlementGateService.shouldShowAds(entitlementState): Boolean` — returns `false` for ACTIVE / TRIAL / GRACE_PERIOD; `true` for NONE / EXPIRED (and for not-yet-authenticated users in free-tier-anonymous flows if those exist). Unit test coverage 100%.
    - `ui/ads/AdBanner.kt` (new package — only consumed by whitelisted screens): Composable that observes `SubscriptionRepository.observeSubscription(userId)` via a scoped ViewModel, and:
      - Renders a `com.google.android.gms.ads.AdView` (via `AndroidView`) with the banner ad unit ID for the current build variant.
      - On `shouldShowAds == false`, cancels any in-flight ad request (`adView.destroy()`) and removes the view from the compose tree within the next recomposition (one-frame guarantee per ISC-015).
    - Placement whitelist (ADR-011 §6) — banner inserted on: Dashboard, Log History list, Standards list. **Not** on: Log Entry form, Calculators, Paywall, Settings, Account, Pool Profile, Alerts UI. Architectural test (`ArchitectureTest` per ADR-010) asserts no `AdBanner` import or `AdView` reference appears in the blocked packages.
    - Build-variant ad-unit-ID separation: `app/src/debug/res/values/ad_ids.xml` and `app/src/release/res/values/ad_ids.xml` each define `<string name="admob_banner_home">...</string>`. Debug and `androidTest` variants resolve to AdMob's published test ad unit IDs (`ca-app-pub-3940256099942544/6300978111` for banner). Release resolves to the production ad unit ID (a `<TBD — owner: user, due: pre-launch Play Console setup>` placeholder until the user provisions it). A CI assertion in `android.yml` greps the APK's generated resources to ensure production ad unit strings never appear in debug/staging outputs.
    - Frequency caps: banner refresh interval defaults to 60s (AdMob default). Configurable via a remote-config-style constant in a future slice if needed; out of scope here.
    - **No interstitials. No rewarded. No native mediation.** Adding any of those requires a new ADR per ADR-011 §6.
  - Verification Step:
    - `./gradlew :app:testDebugUnitTest --tests "*EntitlementGateServiceTest*"` — covers `shouldShowAds` for all five entitlement states.
    - `./gradlew :app:testDebugUnitTest --tests "*ArchitectureTest*"` — asserts ad-composable placement boundary.
    - `./gradlew :app:connectedDebugAndroidTest --tests "*AdBannerTest*"` — simulates entitlement transition NONE → ACTIVE, asserts banner removed within one frame (ISC-015).
    - Manual smoke: run debug build, confirm test banner loads on Dashboard; toggle subscription state (via debug menu or Firestore emulator) and confirm banner disappears immediately.
    - CI check: grep production ad unit ID out of debug APK resources.
  - Human Check Needed?: Yes — (1) user must provision the production AdMob ad unit ID in the Google AdMob console and drop it into `release/res/values/ad_ids.xml` before release builds ship, (2) user must confirm Play Console Data Safety declarations enumerate the AdMob SDK data collection surface, (3) ADR-011 §6 Data Safety declarations and child-directed posture must be reviewed at Play Console launch.

  ---
