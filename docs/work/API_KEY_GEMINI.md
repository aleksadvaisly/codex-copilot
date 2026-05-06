# API Key Gemini

## Cel

Zaplanować dodanie natywnej integracji **Gemini API key (beta)** jako jednej z metod uwierzytelniania w Codex TUI/CLI/app-server, z:

- logowaniem przez podanie klucza albo wybór wykrytej zmiennej środowiskowej,
- dynamicznym wyborem modeli na podstawie tego, co zwraca Gemini API,
- zachowaniem obecnej integracji Gemini i Anthropic przez GitHub Copilot bez regresji.

Ten dokument opisuje **plan wdrożenia**, nie implementację.

## Co jest dziś w repo

### 1. Obecne `WireApi::Gemini` nie jest natywnym Google Gemini API

Aktualna ścieżka Gemini w repo jest zrobiona jako wariant Copilotowy:

- `codex-rs/codex-api/src/endpoint/gemini.rs`
  - `GeminiClient` to cienki wrapper nad `AnthropicClient`.
- `codex-rs/core/src/client.rs`
  - `stream_gemini()` wysyła request na `chat/completions`, tak samo jak ścieżka Anthropic przez Copilot.
- `codex-rs/models-manager/src/manager.rs`
  - GitHub Copilot ma osobny fetch `/models`.
- `codex-rs/models-manager/src/copilot_models.rs`
  - translacja modeli Copilota rozpoznaje rodziny modeli, w tym Gemini.

Wniosek: obecne repo ma już rozdzielenie "Gemini vs Anthropic" na poziomie wyboru wire path, ale **semantyka Gemini jest dziś Copilot-specific**, nie Google-native.

### 2. API-key login jest dziś globalny, nie provider-specific

Obecna ścieżka API key:

- `codex-rs/app-server-protocol/src/protocol/v2.rs`
  - `LoginAccountParams::ApiKey { api_key }`
- `codex-rs/app-server/src/codex_message_processor.rs`
  - zapisuje pojedynczy API key przez `login_with_api_key(...)`
- `codex-rs/login/src/auth/manager.rs`
  - logika storage dotyczy globalnego `OPENAI_API_KEY` / `AuthMode::ApiKey`
- `codex-rs/tui/src/onboarding/auth.rs`
  - onboardingowy ekran API key jest napisany pod OpenAI i prefilluje tylko `OPENAI_API_KEY`

Wniosek: to nie wystarczy do dobrego wsparcia Gemini API key, bo:

- nie ma rozróżnienia "czyj" to API key,
- jeden globalny klucz nadpisywałby inny provider,
- `Account::ApiKey {}` i `AuthMode::ApiKey` nie mówią, czy chodzi o OpenAI czy Gemini,
- onboarding jest OpenAI-centric zarówno w copy, jak i w wykrywaniu env vars.

### 3. Provider config już wspiera env-based auth

`codex-rs/model-provider-info/src/lib.rs` już ma mechanizmy, które warto wykorzystać:

- `env_key`
- `env_key_instructions`
- `wire_api`
- `requires_openai_auth`
- `auth`
- `http_headers` / `env_http_headers`

To jest dobry fundament pod provider Gemini, ale obecne `requires_openai_auth: bool` i globalna ścieżka `ApiKey` są zbyt ogólne dla docelowego UX.

## Co mówi oficjalne Gemini API

Na podstawie oficjalnych źródeł Google AI:

- autoryzacja REST idzie przez nagłówek `x-goog-api-key`,
- standardowy endpoint listy modeli to `GET https://generativelanguage.googleapis.com/v1beta/models`,
- endpointy generacji to `generateContent` i `streamGenerateContent`,
- oficjalnie wspierane env vars w dokumentacji to `GEMINI_API_KEY` i `GOOGLE_API_KEY`,
- dokumentacja SDK mówi, że gdy ustawione są oba, `GOOGLE_API_KEY` ma precedence.

Źródła:

- https://ai.google.dev/api
- https://ai.google.dev/api/models#v1beta.models.list
- https://ai.google.dev/gemini-api/docs/api-key
- https://ai.google.dev/gemini-api/docs/function-calling

## Konsultacja z Gemini

Przez `team prompt --model gemini` dostałem trzy wartościowe sygnały:

1. dynamiczny fetch modeli z Gemini API powinien być częścią planu od początku,
2. onboarding musi mieć osobny flow "Gemini API key",
3. trzeba uważać na różnice między wspólnym modelem rozmowy w Codex a natywnym payloadem Gemini.

Jednocześnie część rekomendacji z konsultacji nie pasuje do obecnego repo i nie powinna być przyjmowana 1:1:

- sugestia tworzenia nowego osobnego secret/keyring subsystemu jest zbyt ciężka,
- sugestia tworzenia od razu nowego osobnego crate tylko na auth/secrets nie pasuje do obecnego wzorca `codex-rs/login`,
- repo ma już istniejące warstwy `login`, `provider_auth`, `model-provider-info`, `models-manager`, więc plan powinien najpierw rozszerzać je, a nie omijać.

## Główne założenie architektoniczne

### Nie wolno "po prostu podmienić" obecnego `GeminiClient`

To jest najważniejsze ryzyko.

Jeśli natywna implementacja Google Gemini zostałaby wpięta bezpośrednio pod obecny `codex-rs/codex-api/src/endpoint/gemini.rs`, to można by łatwo zepsuć działające dziś flow Gemini-via-Copilot.

Dlatego plan musi najpierw rozdzielić dwa przypadki:

- **Gemini przez GitHub Copilot**
- **Gemini natywny przez Google API key**

## Rekomendowany kierunek

### 1. Dodać nowy built-in provider dla natywnego Gemini

Nie przeciążać `github-copilot`.

Proponowany nowy provider:

- `model_provider_id = "gemini-api-beta"` albo `model_provider_id = "google-gemini-beta"`
- `base_url = "https://generativelanguage.googleapis.com"`
- dedykowany opis i copy w UI

Powód:

- provider ma być jawny,
- łatwiej utrzymać oddzielne modele, auth i UX,
- łatwiej uniknąć mieszania stanu Copilota z natywnym Gemini.

### 2. Rozdzielić Copilot Gemini od native Gemini na poziomie transportu

Są dwie opcje:

#### Opcja A, rekomendowana

Zostawić `WireApi::Gemini`, ale rozdzielić wykonanie po providerze:

- provider `github-copilot` używa obecnego copilotowego `chat/completions`,
- provider `gemini-api-beta` używa nowego natywnego request/stream path.

To jest najmniejsza zmiana semantyczna i najmniejsze ryzyko regresji.

#### Opcja B

Wprowadzić nowy jawny wire path dla Copilotowego chat-completions i zarezerwować `WireApi::Gemini` tylko dla natywnego Google API.

To jest czystsze semantycznie, ale większe i bardziej inwazyjne.

Na dziś polecam **Opcję A**.

## Jak powinno działać uwierzytelnianie

### Wymaganie produktowe

Jedna z metod logowania ma być:

- **Gemini API key (beta)**

oraz:

- jeśli nie znaleziono klucza w env, prosimy o ręczne podanie,
- jeśli znaleziono więcej niż jedną sensowną zmienną środowiskową, pokazujemy menu wyboru,
- wybór modelu bierze się z tego, co zwraca API.

### Kandydaci env vars

Na bazie oficjalnych docs Google:

- `GEMINI_API_KEY`
- `GOOGLE_API_KEY`

Na starcie nie rekomenduję skanowania szerzej niż ta jawna lista.

### Zachowanie onboardingu

#### Gdy znaleziono 0 kluczy

Pokazać:

- pole do wklejenia klucza,
- hint o wspieranych env vars,
- link/instrukcję pozyskania klucza.

#### Gdy znaleziono 1 klucz

Pokazać:

- "Use detected key from `GEMINI_API_KEY`" albo `GOOGLE_API_KEY`,
- opcję "Paste another key",
- opcjonalnie krótkie masked preview końcówki wartości.

#### Gdy znaleziono >1 różne klucze

Pokazać menu:

- `Use GEMINI_API_KEY`
- `Use GOOGLE_API_KEY`
- `Paste another key`

Tu nie polecam cichego precedence. W interaktywnym UX lepszy jest jawny wybór.

### Zachowanie nieinteraktywne / CLI

Tu warto mieć regułę deterministyczną.

Rekomendacja:

- jeśli użytkownik jawnie poda źródło, użyć go,
- jeśli nie poda i jest jeden kandydat, użyć go,
- jeśli nie poda i są dwa różne kandydaty, zwrócić czytelny błąd albo wymusić wybór,
- nie polegać ślepo na precedence SDK Google, bo Codex ma własny UX i własny storage.

## Kluczowa zmiana danych: provider-scoped API keys

Obecny model "jeden globalny `ApiKey`" jest za słaby.

Docelowo potrzebujemy provider-scoped storage, np.:

- `provider_api_keys["openai"] = ...`
- `provider_api_keys["gemini-api-beta"] = ...`

Nie rekomenduję dokładania osobnego subsystemu secrets. Lepiej rozszerzyć obecny storage w `codex-rs/login`.

### Dlaczego to potrzebne

- użytkownik może mieć równolegle OpenAI API key i Gemini API key,
- przełączanie providerów nie powinno kasować kluczy,
- `provider_auth.rs` musi umieć rozwiązać auth dla konkretnego providera,
- onboarding i CLI muszą być provider-aware.

### Konsekwencje

Do rozważenia w planie implementacji:

- rozszerzenie `AuthDotJson` o provider-scoped klucze,
- albo osobny provider-auth blob w ramach istniejącego storage backendu.

Nie rekomenduję zostawienia tego jako jednego `OPENAI_API_KEY` pola z heurystyką.

## Model discovery

### Wymaganie

Lista modeli dla provider `gemini-api-beta` ma pochodzić z Gemini API, nie z hardcode.

### Plan

W `codex-rs/models-manager/src/manager.rs` dodać natywny fetch modeli:

- `GET /v1beta/models`
- paginacja po `pageToken`
- filtrowanie modeli po wspieranych akcjach, przede wszystkim `generateContent`

### Co tłumaczyć do `ModelInfo`

Z odpowiedzi API warto przenieść przynajmniej:

- `name`
- `baseModelId`
- `displayName`
- `description`
- `inputTokenLimit`
- `outputTokenLimit`
- `supported_actions`

Nie trzeba od razu mapować wszystkich capability flags. Pierwsza iteracja może ograniczyć się do:

- czy model nadaje się do zwykłego turnu,
- limity tokenów,
- nazwa/slug do wyboru w pickerze.

### Cache

Dodać osobny cache modeli dla tego providera, analogicznie jak repo robi to dziś dla Copilota.

## Streaming i request shape

### Co mamy dziś

Dzisiejszy `GeminiClient` używa copilotowego `chat/completions`.

### Co trzeba zrobić dla native Gemini

Wprowadzić nowy natywny path request/stream:

- `generateContent`
- `streamGenerateContent`

oraz osobny serializer/deserializer dla google’owego formatu:

- `contents`
- `parts`
- config specyficzny dla Gemini
- mapowanie tool calling

### Wniosek praktyczny

Najpierw trzeba zrobić **native Gemini client path**, a dopiero potem podpiąć onboarding i model picker.

Inaczej powstanie UI, które loguje użytkownika do providera, którego runtime jeszcze nie umie poprawnie obsłużyć.

## Wpływ na app-server protocol

To jest jedna z ważniejszych decyzji.

### Problem

Dziś protokół ma tylko:

- `LoginAccountParams::ApiKey { api_key }`
- `LoginAccountResponse::ApiKey {}`
- `Account::ApiKey {}`
- `AuthMode::ApiKey`

To nie niesie informacji, dla jakiego providera zapisujemy klucz.

### Rekomendacja

Rozszerzyć protokół o provider-awareness, np.:

- `LoginAccountParams::ApiKey { provider_id, api_key }`
- `LoginAccountResponse::ApiKey { provider_id }`
- `Account::ApiKey { provider_id: Option<String> }`

Nie trzeba od razu zmieniać semantyki `AuthMode`, ale UI i status powinny móc powiedzieć:

- "API key: OpenAI"
- "API key: Gemini API (beta)"

Bez tego onboarding i status będą nieczytelne.

## Wpływ na TUI onboarding

### Dziś

Obecny screen miesza:

- ChatGPT,
- GitHub Copilot,
- Device Code,
- API key dla OpenAI.

### Docelowo

Metody logowania powinny być renderowane zależnie od aktywnego providera albo klasy providera.

Dla `gemini-api-beta` onboarding powinien pokazać coś w rodzaju:

- `Use Gemini API key (beta)`
- podflow: env detection / env selection / manual paste

Nie polecam dokładania "Gemini API key" jako kolejnej stałej globalnej pozycji zawsze dla każdego providera, bo picker szybko zamieni się w nieczytelne menu.

### Rekomendacja UX

Rozdzielić:

- globalny screen onboardingowy,
- provider-specific auth method screen.

Minimalnie:

- gdy aktywny provider to Gemini native, karta API-key dostaje Gemini copy i Gemini env detection,
- gdy aktywny provider to OpenAI, zostaje obecna ścieżka,
- gdy aktywny provider to GitHub Copilot, zostaje obecna ścieżka device-code.

## Wpływ na CLI

CLI powinno wspierać ten sam model myślenia co TUI.

### Rekomendacja

Nie rozbudowywać tylko obecnego:

- `codex login --with-api-key`

bez kontekstu providera.

Lepsze opcje:

- infer provider z aktywnego configu,
- albo dodać jawny `--provider gemini-api-beta`,
- albo oba.

Minimalny sensowny wariant:

- jeśli aktywny provider to Gemini native, `--with-api-key` zapisuje klucz dla Gemini,
- jeśli aktywny provider to OpenAI, zapisuje dla OpenAI,
- jeśli provider nie wspiera stored API key login, CLI zgłasza błąd.

## Plan wdrożenia

### Faza 0: decyzje architektoniczne

1. Wybrać provider id dla native Gemini.
2. Potwierdzić rozdzielenie "Gemini via Copilot" od "native Gemini".
3. Zdecydować format provider-scoped API key storage.
4. Zdecydować, czy app-server protocol od razu dostaje `provider_id`.

### Faza 1: provider i runtime

1. Dodać built-in provider `gemini-api-beta`.
2. Dodać native Gemini request path w `codex-api` / `core`.
3. Dodać native model discovery z `/v1beta/models`.
4. Dodać mapowanie modeli do `ModelInfo`.

Warunek zakończenia:

- da się użyć Gemini native bez TUI onboarding, np. przez env/config i ręczny wybór providera.

### Faza 2: auth storage i provider-aware auth resolution

1. Rozszerzyć login storage o provider-scoped API keys.
2. Rozszerzyć `provider_auth.rs` / `api_bridge.rs`, żeby wybierały właściwy klucz dla provider-a.
3. Rozszerzyć app-server login flow o provider-aware API key path.

Warunek zakończenia:

- OpenAI API key i Gemini API key mogą współistnieć.

### Faza 3: TUI onboarding

1. Dodać metodę `Gemini API key (beta)` dla provider-a Gemini.
2. Dodać env scan:
   - `GEMINI_API_KEY`
   - `GOOGLE_API_KEY`
3. Dodać menu wyboru źródła, gdy wykryto >1 różne wartości.
4. Dodać flow manual paste.
5. Dodać snapshot tests dla nowych ekranów.

Warunek zakończenia:

- świeży start aplikacji potrafi przeprowadzić użytkownika od braku auth do używalnej sesji Gemini.

### Faza 4: status, account/read, polish

1. Pokazać provider-aware stan zalogowania.
2. Dostosować copy w status line / welcome / login success.
3. Dodać telemetry bez wycieku sekretów.
4. Dodać regresyjne testy przełączania providerów.

## Ryzyka

### 1. Zbyt duże przeciążenie obecnego `WireApi::Gemini`

Jeśli native Gemini i Copilot Gemini będą dalej dzielić jeden client bez jawnego rozdziału, regresja jest bardzo prawdopodobna.

### 2. Za słaby model storage

Jeśli zostaniemy przy jednym globalnym API key, UX przełączania providerów będzie zły i nieprzewidywalny.

### 3. Przedwczesne wejście w UI bez gotowego runtime

Najpierw runtime/model-list, potem login UX.

### 4. Ciche precedence env vars

W TUI nie warto milcząco wybierać jednego klucza, gdy wykryto kilka różnych.

## Konkretne miejsca w repo do ruszenia

Najbardziej prawdopodobne touchpointy:

- `codex-rs/model-provider-info/src/lib.rs`
- `codex-rs/codex-api/src/endpoint/gemini.rs`
- `codex-rs/core/src/client.rs`
- `codex-rs/models-manager/src/manager.rs`
- `codex-rs/login/src/auth/manager.rs`
- `codex-rs/login/src/provider_auth.rs`
- `codex-rs/login/src/api_bridge.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `codex-rs/app-server/src/codex_message_processor.rs`
- `codex-rs/tui/src/onboarding/auth.rs`
- `codex-rs/cli/src/login.rs`

## Rekomendacja końcowa

Najbezpieczniejszy plan to:

1. potraktować native Gemini jako **nowy provider**,
2. nie ruszać semantyki obecnego `github-copilot`,
3. dodać **provider-scoped API key storage**,
4. zrobić **dynamiczny model discovery z Gemini API**,
5. dopiero potem dołożyć onboardingową metodę **Gemini API key (beta)** z:
   - wykrywaniem `GEMINI_API_KEY` / `GOOGLE_API_KEY`,
   - menu wyboru przy wieloznaczności,
   - ręcznym wpisaniem klucza jako fallback.

To podejście minimalizuje ryzyko regresji w działającym dziś Copilocie i pozwala dowieźć Gemini native etapami, bez przepisywania połowy auth stacku na raz.
