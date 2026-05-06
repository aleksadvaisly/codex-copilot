# Gemini API Key Plan

## Cel

Wdrożyć natywny provider Gemini oparty o Google Gemini API key, bez regresji dla
obecnego Gemini-via-Copilot.

Ten dokument jest wykonawczym planem implementacji dla decyzji opisanych w
`docs/work/API_KEY_GEMINI.md`.

## Ustalone decyzje

- natywny Gemini wchodzi jako **nowy provider**
- nie zmieniamy semantyki obecnego `github-copilot`
- onboarding ma wspierać `Gemini API key (beta)`
- model picker dla tego providera ma bazować na odpowiedzi `GET /v1beta/models`
- interaktywny login ma wspierać:
  - ręczne wklejenie klucza
  - użycie klucza z `GEMINI_API_KEY`
  - użycie klucza z `GOOGLE_API_KEY`
  - jawny wybór, gdy wykryto więcej niż jedną różną wartość

## Proponowana nazwa providera

Rekomenduję:

- `model_provider_id = "gemini-api-beta"`

Powody:

- jest krótki i jednoznaczny
- odróżnia się od `github-copilot`
- zostawia miejsce na przyszłe `gemini-api` bez suffixu `-beta`, jeśli produkt
  dojrzeje

## Status Legend

- `[x]` zrobione i lokalnie zweryfikowane
- `[ ]` niezrobione albo częściowo zrobione

## Faza 0: decyzje i kontrakty

### TASK-001: Zarejestrować provider ID `gemini-api-beta`

- [ ] dodać built-in provider metadata w
  `codex-rs/model-provider-info/src/lib.rs`
- [ ] ustawić provider display name, description i `base_url`
- [ ] przypisać provider do `WireApi::Gemini`, ale bez ruszania zachowania
  `github-copilot`
- [ ] zarezerwować `gemini-api-beta` w walidacji configu, jeśli repo ma listę
  reserved built-ins

Definition of done:

- provider da się wskazać w configu bez hacków
- walidacja nie traktuje go jako zwykłego custom providera

Verification:

- `cargo test -p codex-model-provider-info`

### TASK-002: Ustalić kontrakt auth dla provider-scoped API key

- [ ] zdecydować docelowy kształt storage w `codex-rs/login`
- [ ] zdecydować docelowy kształt app-server protocol dla API-key login
- [ ] potwierdzić, które warstwy muszą znać `provider_id`, a które tylko
  wynik rozwiązanego auth

Rekomendacja:

- w protokole przejść na `LoginAccountParams::ApiKey { provider_id, api_key }`
- w storage trzymać klucze per provider zamiast jednego globalnego `ApiKey`

Definition of done:

- istnieje jeden zaakceptowany kształt danych dla storage i dla app-server

Verification:

- aktualizacja tego dokumentu lub komentarz w kodzie z przyjętym kontraktem

## Faza 1: runtime native Gemini

### TASK-003: Dodać natywny klient Gemini request/stream

- [ ] rozdzielić w `codex-rs/codex-api/src/endpoint/gemini.rs` ścieżkę native
  Gemini od obecnego wrappera copilotowego
- [ ] jeśli plik zacznie puchnąć, wyciągnąć native implementację do osobnego
  modułu zamiast rozbudowywać obecny plik
- [ ] dodać request builder dla:
  - `POST /v1beta/models/{model}:generateContent`
  - `POST /v1beta/models/{model}:streamGenerateContent`
- [ ] ustawić auth przez nagłówek `x-goog-api-key`
- [ ] dodać serializer dla Google Gemini `contents` / `parts`
- [ ] dodać parser odpowiedzi i zdarzeń streamu do wspólnego modelu eventów

Definition of done:

- `gemini-api-beta` potrafi wykonać zwykły turn tekstowy bez TUI onboarding

Verification:

- `cargo test -p codex-api`
- `cargo test -p codex-core`

### TASK-004: Nie zepsuć Gemini-via-Copilot

- [ ] utrzymać obecną ścieżkę `github-copilot` bez zmiany endpointu
- [ ] dopilnować, żeby provider `github-copilot` dalej używał
  copilotowego `chat/completions`
- [ ] dodać lub rozszerzyć test rozdzielający oba warianty

Definition of done:

- dwa providery o tym samym `WireApi::Gemini` wykonują różne ścieżki transportu
  zależnie od provider id

Verification:

- test routingowy w `codex-core` albo `codex-api`

### TASK-005: Dodać mapowanie model discovery z Gemini API

- [ ] w `codex-rs/models-manager/src/manager.rs` dodać fetch listy modeli dla
  provider `gemini-api-beta`
- [ ] obsłużyć paginację po `pageToken`
- [ ] filtrować modele po capability potrzebnej do zwykłych turnów, przede
  wszystkim `generateContent`
- [ ] przemapować co najmniej:
  - `name`
  - `displayName`
  - `description`
  - `inputTokenLimit`
  - `outputTokenLimit`
  - `supportedActions`
- [ ] dodać provider-specific cache filename

Definition of done:

- po refreshu modeli picker dostaje listę modeli z Gemini API zamiast hardcode

Verification:

- `cargo test -p codex-models-manager`

### TASK-006: Zdefiniować politykę pierwszego MVP dla capability

- [ ] ustalić, czy MVP obsługuje tylko zwykły tekstowy turn
- [ ] jeśli tool calling nie wchodzi do MVP, zapisać to jawnie w kodzie i docs
- [ ] jeśli tool calling wchodzi, dodać mapowanie function calling dla Gemini

Rekomendacja MVP:

- najpierw zwykły tekstowy turn i model discovery
- function calling jako osobny etap po uruchomieniu bazowego flow

Definition of done:

- repo ma jasny, testowalny scope pierwszej iteracji

## Faza 2: auth storage i provider-aware resolution

### TASK-007: Rozszerzyć storage o provider-scoped API keys

- [ ] zmienić `codex-rs/login/src/auth/manager.rs`, żeby wspierał klucze per
  provider
- [ ] zadbać o migrację albo kompatybilność wsteczną dla istniejącego
  OpenAI-centric storage
- [ ] upewnić się, że logout/login nie kasuje klucza innego providera

Definition of done:

- OpenAI API key i Gemini API key mogą współistnieć

Verification:

- `cargo test -p codex-login`

### TASK-008: Zmienić provider auth resolution

- [ ] zaktualizować `codex-rs/login/src/provider_auth.rs`
- [ ] zaktualizować `codex-rs/login/src/api_bridge.rs`
- [ ] dopilnować, żeby runtime dla `gemini-api-beta` najpierw sprawdzał:
  - klucz zapisany dla providera
  - wybrane źródło env
  - config providerowy, jeśli repo już ma taki fallback
- [ ] dopilnować, żeby `github-copilot` nie wpadał w ścieżkę Gemini API key

Definition of done:

- auth resolution jest provider-aware i deterministyczny

Verification:

- `cargo test -p codex-login`
- `cargo test -p codex-core`

### TASK-009: Rozszerzyć app-server protocol o provider-aware API key login

- [ ] zmienić `codex-rs/app-server-protocol/src/protocol/v2.rs`
- [ ] dodać `provider_id` do `LoginAccountParams::ApiKey`
- [ ] dodać `provider_id` do odpowiedzi i ewentualnie account/status payloadów
- [ ] zaktualizować `codex-rs/app-server/src/codex_message_processor.rs`
- [ ] zaktualizować README lub docs dla app-server, jeśli to publiczny kontrakt

Definition of done:

- app-server umie zapisać API key dla wskazanego providera

Verification:

- `cargo test -p codex-app-server-protocol`
- `cargo test -p codex-app-server`

## Faza 3: TUI onboarding i login UX

### TASK-010: Dodać provider-specific login method dla Gemini

- [ ] rozszerzyć `codex-rs/tui/src/onboarding/auth.rs`
- [ ] gdy aktywny provider to `gemini-api-beta`, pokazać metodę:
  - `Gemini API key (beta)`
- [ ] nie dokładać tej metody jako globalnej pozycji dla wszystkich providerów

Definition of done:

- użytkownik wybierający provider Gemini widzi właściwą metodę logowania

Verification:

- `cargo test -p codex-tui`

### TASK-011: Dodać env detection i source picker

- [ ] wykrywać `GEMINI_API_KEY`
- [ ] wykrywać `GOOGLE_API_KEY`
- [ ] rozpoznawać przypadki:
  - brak kluczy
  - jeden klucz
  - wiele różnych kluczy
- [ ] dodać ekran/menu źródła:
  - use `GEMINI_API_KEY`
  - use `GOOGLE_API_KEY`
  - paste another key
- [ ] pokazać masked preview wartości tylko jeśli repo ma już bezpieczny wzorzec
  prezentacji sekretów

Definition of done:

- użytkownik nie musi ręcznie wpisywać klucza, jeśli sensowny env już istnieje

Verification:

- `cargo test -p codex-tui`

### TASK-012: Dodać manual paste flow i success path

- [ ] dodać ekran wpisania klucza
- [ ] zwalidować minimalnie pusty/oczywiście zły input
- [ ] po zapisaniu auth przejść do odświeżenia modeli dla `gemini-api-beta`
- [ ] po sukcesie przejść do model pickera albo welcome screen, zależnie od
  obecnego flow TUI

Definition of done:

- nowy użytkownik może dojść od pustego startu do aktywnej sesji Gemini

Verification:

- `cargo test -p codex-tui`

### TASK-013: Zaktualizować welcome screen po logout i nowym wejściu

- [ ] dopilnować, żeby po `logout` dla `gemini-api-beta` TUI wracał do welcome
  screen z prośbą o logowanie
- [ ] dopilnować, żeby screen nie wpadał z powrotem w copy GitHub Copilot
- [ ] jeśli logout jest globalny, określić czy ma wylogowywać tylko aktywny
  provider czy wszystkie stored credentials

Powiązanie:

- to zadanie domyka wcześniejszy problem UX, który już zgłaszałeś dla logout

Definition of done:

- po restarcie aplikacji użytkownik bez ważnego auth trafia na poprawny welcome
  screen dla aktywnego providera

Verification:

- `cargo test -p codex-tui`
- snapshot tests dla welcome/auth flow

### TASK-014: Dodać snapshot tests dla nowych ekranów

- [ ] dodać snapshot dla:
  - wyboru metody `Gemini API key (beta)`
  - ekranu bez env vars
  - ekranu z jednym env var
  - ekranu z dwoma różnymi env vars
  - success state albo przejścia do model pickera
- [ ] zaakceptować snapshoty po review

Definition of done:

- UI impact jest pokryty snapshotami i reviewowalny

Verification:

- `cargo test -p codex-tui`
- `cargo insta pending-snapshots -p codex-tui`

## Faza 4: CLI i status UX

### TASK-015: Uczynić `codex login --with-api-key` provider-aware

- [ ] zaktualizować `codex-rs/cli/src/login.rs`
- [ ] jeśli provider jest `gemini-api-beta`, logować dla Gemini
- [ ] jeśli provider nie jest jednoznaczny, dopuścić albo wymusić
  `--provider gemini-api-beta`
- [ ] przy dwóch różnych env vars zwracać czytelny błąd albo żądać jawnego
  wyboru

Definition of done:

- CLI zachowuje się deterministycznie dla Gemini API key

Verification:

- `cargo test -p codex-cli`

### TASK-016: Zaktualizować status i account display

- [ ] w TUI/CLI/app-server pokazać, że zapisany API key dotyczy konkretnego
  providera
- [ ] copy typu `Logged in with API key` rozszerzyć do postaci provider-aware
- [ ] dopilnować, żeby status nie sugerował OpenAI, gdy aktywny jest Gemini

Definition of done:

- użytkownik widzi, czy używa OpenAI API key czy Gemini API key

Verification:

- `cargo test -p codex-cli`
- `cargo test -p codex-tui`
- `cargo test -p codex-app-server`

## Faza 5: testy, docs i rollout

### TASK-017: Dodać testy regresyjne provider switching

- [ ] przypadek: OpenAI key zapisany, potem Gemini key zapisany
- [ ] przypadek: Gemini logout nie kasuje OpenAI key
- [ ] przypadek: `github-copilot` dalej działa po dodaniu native Gemini
- [ ] przypadek: model refresh fallback działa przy błędzie Gemini `/models`

Definition of done:

- najważniejsze ścieżki współistnienia providerów są pokryte testami

Verification:

- `cargo test -p codex-login`
- `cargo test -p codex-models-manager`
- `cargo test -p codex-core`

### TASK-018: Uzupełnić dokumentację produktu i implementacji

- [ ] zaktualizować `docs/work/API_KEY_GEMINI.md` o status realizacji
- [ ] dopisać sekcję known limitations dla MVP
- [ ] jeśli zmieni się publiczny kontrakt app-server, zaktualizować odpowiednie
  README/docs

Definition of done:

- dokumentacja odzwierciedla realny stan implementacji

## Kolejność realizacji

- [ ] 1. `TASK-001`
- [ ] 2. `TASK-002`
- [ ] 3. `TASK-003`
- [ ] 4. `TASK-004`
- [ ] 5. `TASK-005`
- [ ] 6. `TASK-007`
- [ ] 7. `TASK-008`
- [ ] 8. `TASK-009`
- [ ] 9. `TASK-010`
- [ ] 10. `TASK-011`
- [ ] 11. `TASK-012`
- [ ] 12. `TASK-013`
- [ ] 13. `TASK-014`
- [ ] 14. `TASK-015`
- [ ] 15. `TASK-016`
- [ ] 16. `TASK-017`
- [ ] 17. `TASK-018`

## Krytyczne zależności

- `TASK-003` zależy od `TASK-001`
- `TASK-005` zależy od `TASK-001`
- `TASK-007` i `TASK-009` zależą od `TASK-002`
- `TASK-010` do `TASK-014` nie powinny startować przed domknięciem
  `TASK-003` i `TASK-005`
- `TASK-015` nie powinien startować przed `TASK-007` i `TASK-008`

## MVP

Pierwsze sensowne MVP to:

- nowy provider `gemini-api-beta`
- tekstowy native turn
- model discovery z `/v1beta/models`
- provider-scoped API key storage
- TUI login przez env albo manual paste

Poza MVP można zostawić:

- pełne function calling dla Gemini
- dodatkowe capability flags ponad zwykły turn
- bardziej zaawansowaną politykę wielokrotnego logoutu dla wielu providerów
