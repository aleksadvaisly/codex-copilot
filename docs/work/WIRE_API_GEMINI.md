# WireApi Gemini

## Cel

Dodać natywne wsparcie dla Gemini jako osobnego wire API w `codex-rs`, analogicznie do `WireApi::Anthropic`.

Ważne doprecyzowanie: chodzi o Gemini dostępne przez GitHub Copilot, nie o osobną subskrypcję Google AI Studio.

Ten dokument opisuje założenia, które chcemy utrzymać przed implementacją:

- Gemini ma być pierwszorzędnym wire API, a nie shimem przez OpenAI Responses.
- Logika wyboru wire API ma pozostać jawna w `ModelProviderInfo`.
- Model discovery i filtracja `models` mają traktować Gemini jako osobną rodzinę modeli.
- Implementacja ma minimalnie naruszać istniejący path dla Responses.

## Założenia

1. **Gemini nie jest Responses shimem**
   - Zakładamy osobny serializer i osobny runtime path.
   - Nie próbujemy mapować wszystkiego 1:1 do OpenAI Responses.

2. **Gemini będzie obok Responses i Anthropic**
   - `WireApi` powinno docelowo mieć co najmniej:
     - `Responses`
     - `Anthropic`
     - `Gemini`
   - Wybór wire API ma wynikać z konfiguracji providera, nie z heurystyki po nazwie modelu.
   - Pierwsza iteracja zakłada Gemini w ramach GitHub Copilot model provider, nie osobny Google auth flow.

3. **Modele Gemini muszą być widoczne w `models`**
   - Jeśli provider Gemini zwraca modele z endpointem Gemini-native, to `supported_in_api` musi to odzwierciedlać.
   - Filtr w `ModelPreset::filter_by_auth` nie powinien zawierać wyjątków typu „sprawdź nazwę modelu”.

4. **Konfiguracja providera musi pozostać spójna**
    - `wire_api = "gemini"` powinno być ustawiane w `config.toml` tak samo jawnie jak `anthropic`.
    - Wsparcie musi działać z istniejącym systemem `model_provider`, `requires_openai_auth`, `auth`, `headers` i `query_params` tam, gdzie to ma sens.
   - Dla Copilota zakładamy, że Gemini jest jednym z endpointów/model families wystawianych przez ten sam provider, więc nie projektujemy osobnego logowania do Google.

5. **Wspólny model rozmowy pozostaje po stronie core**
   - Nie chcemy rozlewać Gemini-specific typów po UI i warstwach wysokiego poziomu.
   - Core powinien nadal operować na wspólnym modelu turnów, a wire adapter ma mapować go na payload Gemini.

6. **Najpierw poprawność, potem optymalizacje**
   - Pierwsza wersja może ograniczać się do minimalnego zestawu funkcji potrzebnego do listowania modeli i wysłania turnu.
   - Tool calling, reasoning hints, multimodal input i streaming mogą być dowożone etapami.

## Co już mamy

Po ostatnich zmianach repo ma już wzorzec, który można skopiować:

- `WireApi::Anthropic` istnieje w `codex-rs/model-provider-info/src/lib.rs`.
- `codex-rs/models-manager/src/copilot_models.rs` potrafi oznaczać modele spoza Responses jako `supported_in_api`.
- `ModelPreset::filter_by_auth` nadal trzyma prostą regułę: w trybie nie-ChatGPT pokazujemy tylko API-supported modele.

To oznacza, że Gemini powinien wejść dokładnie tym samym stylem:

- provider mówi, jaki wire API obsługuje
- model discovery oznacza, czy model jest używalny przez aktualny wire path
- UI i `model/list` nie zgadują tego same

## Proponowana architektura

### 1. Rozszerzyć `WireApi`

W `codex-rs/model-provider-info/src/lib.rs` dodać:

- `WireApi::Gemini`

oraz:

- serde parsing i `Display`
- testy deserializacji konfiguracji providera
- aktualizację dokumentacji config/schema, jeśli potrzebna

### 2. Dodać Gemini-native client path

W `codex-rs/core/src/client.rs` albo w wydzielonym module obok niego dodać osobny path dla Gemini.

Założenie:

- Responses zostaje bez zmian
- Anthropic zostaje bez zmian
- Gemini dostaje własny serializer i stream processor
- Jeśli Copilot wystawia Gemini przez własny gateway, adapter mapuje to jako native Gemini wire API, ale auth nadal idzie przez Copilot/provider credentials

### 3. Zmienić model discovery

Jeśli Gemini provider ma własne endpointy albo własny `/models` format, to:

- `models-manager` musi rozpoznawać je jako `supported_in_api`
- `model/list` musi je przepuszczać dla właściwej sesji/auth
- TUI powinno widzieć je automatycznie przez istniejący pipeline

### 4. Zachować izolację w UI

UI nie powinno znać szczegółów wire API poza rzeczami typu:

- czy model wspiera reasoning controls
- czy model wspiera websockets/streaming
- czy model jest dostępny w pickerze

## Zakres pierwszej iteracji

Najrozsądniejszy minimalny zakres:

1. `WireApi::Gemini`
2. provider config parsing
3. Gemini model discovery
4. podstawowy streaming turnów
5. test jednostkowy dla tłumaczenia modeli
6. test integracyjny dla `model/list` lub e2e z mockiem, jeśli mamy łatwy fixture

## Otwarte pytania

- Jak dokładnie Gemini ma mapować system prompt, assistant history i tool calls?
- Czy Gemini ma wspierać ten sam zestaw funkcji co Anthropic od razu, czy tylko podzbiór?
- Czy provider Gemini ma korzystać z tych samych auth primitives co istniejące providery Copilot, czy wymaga osobnej logiki tokenów?
- Czy Gemini będzie dostępny tylko dla modeli z własnego provider config Copilot, czy też jako mapowanie części modeli z obecnych zewnętrznych katalogów?

- Jak dokładnie Copilot oznacza modele Gemini w `/models`, żeby `supported_in_api` można było ustalać bez heurystyk po nazwie?
- Jakie są docelowe endpointy Gemini w Copilot gateway i czy różnią się od standardowego Google Gemini API?

## Kryteria ukończenia

Uważamy plan za gotowy, gdy:

- `WireApi::Gemini` jest jawnie dostępne w konfiguracji providera.
- Dokument i implementacja wyraźnie traktują Gemini jako część Copilot model provider, nie niezależną subskrypcję Google.
- `models` pokazuje Gemini-native modele, jeśli provider je zwraca.
- Gemini ma osobny path streamingowy w core.
- Testy potwierdzają, że Gemini nie jest filtrowane jak zwykły OpenAI Responses model.

## Uwaga

Ten plan zakłada, że Gemini ma być dodane jako native wire API, a nie jako specjalny przypadek w filtrze modeli. Jeśli później okaże się, że potrzebny jest inny kształt endpointów lub auth, zmieniamy tylko adapter, nie wspólny model turnów.
