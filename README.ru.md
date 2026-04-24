# Realtime Call Translator

[![English version](https://img.shields.io/badge/lang-english-blue)](README.md)

Переводчик речи в реальном времени для видео- и голосовых звонков. Переводит обе стороны разговора на лету: вы говорите на своём языке, собеседник слышит на своём, и наоборот.

**Как это работает:** Аудио с микрофона проходит через распознавание речи (STT), переводится LLM, синтезируется обратно в речь (TTS) и направляется в звонок. То же самое происходит в обратную сторону для аудио собеседника.

Поддерживает **29 языков** с STT, переводом и TTS. Голосовые модели от [Piper](https://github.com/rhasspy/piper) — скачиваются прямо из веб-интерфейса.

![macOS](https://img.shields.io/badge/platform-macOS_14+-lightgrey)
![License](https://img.shields.io/badge/license-MIT-blue)

> **Платформы:** macOS 14+, Windows 10/11, Linux
> - macOS: Использует CoreAudio и cpal для захвата аудио
> - Windows: Использует WASAPI через cpal (автоматически)
> - Linux: Использует PulseAudio/JACK через cpal

---

## Быстрый старт

**Установка одной командой** (macOS с Homebrew):

```bash
git clone https://github.com/LetovKai/call-translator.git
cd call-translator
./setup.sh
```

Скрипт устанавливает все зависимости, скачивает голосовые модели для английского и русского, и собирает проект.

Далее:

```bash
./run.sh
```

Откройте **http://127.0.0.1:5050** в **Google Chrome**. При первом запуске настройки откроются автоматически — введите API-ключи и выберите языки.

> **Браузер:** Используйте **Chrome** — аудио-монитор и маршрутизация через BlackHole работают корректно. В Safari монитор не работает из-за ограничений аудио-выхода. Другие браузеры не тестировались.

> Нужны два бесплатных API-ключа:
> - [Deepgram](https://console.deepgram.com) — распознавание речи
> - [Groq](https://console.groq.com) — перевод (LLM)

---

## Архитектура

```
┌─────────────┐     ┌──────────────┐     ┌───────────┐     ┌─────────┐
│  Ваш микро- │────>│ Deepgram STT │────>│ Groq LLM  │────>│ Piper   │──> Звонок
│  фон (рус.) │     │ (речь→текст) │     │ (перевод) │     │  TTS    │  (BlackHole)
└─────────────┘     └──────────────┘     └───────────┘     └─────────┘

┌─────────────┐     ┌──────────────┐     ┌───────────┐     ┌─────────┐
│   Аудио     │────>│ Deepgram STT │────>│ Groq LLM  │────>│ Piper   │──> Динамики
│  звонка     │     │ (речь→текст) │     │ (перевод) │     │  TTS    │
└─────────────┘     └──────────────┘     └───────────┘     └─────────┘
```

- **Elixir** — оркестратор, супервизия процессов, управление портами
- **Rust** — захват/воспроизведение аудио, STT-стриминг, синтез речи, перевод
- **Flask** — веб-интерфейс для транскрипта, настроек и управления

---

## Требования

| Зависимость | Назначение | Установка (macOS) | Установка (Windows) |
|---|---|---|---|
| Elixir | Рантайм приложения | `brew install elixir` | [Chocolatey](https://chocolatey.org/packages/elixir): `choco install elixir` |
| Rust | Аудио-движок | `rustup init` | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | rustup init` |
| Python 3 | Веб-сервер UI | `brew install python@3` | [Python.org](https://python.org) или winget: `winget install Python.Python.3` |
| espeak-ng | Фонемизация для TTS | `brew install espeak-ng` | winget: `winget install espeak-ng` |
| ONNX Runtime | Инференс моделей | `brew install onnxruntime` | winget или скачать с [Microsoft](https://learn.microsoft.com/en-us/azure/machine-learning/reference/onnx-runtime)
| Flask | Веб-фреймворк | через venv (см. ниже) | через venv (то же, что на macOS) |
| WASAPI | Аудио I/O | N/A | Встроено в Windows

**API-ключи (есть бесплатные тарифы):**
- [Deepgram](https://console.deepgram.com) — распознавание речи (модель Nova-3)
- [Groq](https://console.groq.com) — перевод через llama-3.3-70b

---

## Ручная установка

Если хотите установить всё пошагово вместо `setup.sh`:

### 1. Системные пакеты

**macOS:**
```bash
xcode-select --install
brew install elixir rustup espeak-ng onnxruntime python@3
rustup-init -y --default-toolchain stable
source ~/.cargo/env

# Виртуальное окружение и Flask
python3 -m venv .venv
source .venv/bin/activate
pip install flask
```

**Windows:**
```powershell
# Установка через winget (рекомендуется)
winget install Elixir.Elixir
rustup-init -y --default-toolchain stable
winget install Python.Python.3
winget install espeak-ng

# ИЛИ использовать Chocolatey:
choco install elixir rust python espeak-ng onnxruntime

# Виртуальное окружение и Flask
python -m venv .venv
.venv\Scripts\activate
pip install flask
```

### 2. Настройка аудио

**macOS (BlackHole):**
1. Скачайте и установите [BlackHole](https://existential.audio/blackhole/)
2. Нужны **оба**:
   - **BlackHole 16ch** — захватывает аудио из приложения для звонков
   - **BlackHole 2ch** — отправляет переведённое аудио обратно в звонок
3. Настройка в приложении для звонков (Google Meet, Zoom и т.д.):
   - Откройте звонок в **Google Chrome**
   - Установите **BlackHole 2ch** как **микрофон** в приложении для звонков
   - Установите **BlackHole 16ch** как **динамики** в приложении для звонков

> **Важно:** НЕ используйте Multi-Output Device — это может вызвать проблемы со звуком.

**Windows (WASAPI):**
Дополнительные драйверы не нужны! Проект использует WASAPI напрямую через cpal:
1. Установите нужный микрофон как устройство ввода по умолчанию
2. Установите свои динамики/наушники как устройство вывода по умолчанию
3. Дополнительная виртуальная маршрутизация не требуется — аудио передаётся напрямую между устройствами

### 3. Голосовые модели

TTS-голоса от [Piper](https://github.com/rhasspy/piper). Скрипт установки автоматически скачивает голоса для английского и русского. Дополнительные голоса можно скачать из веб-интерфейса — выберите язык и нажмите кнопку загрузки.

Для ручной загрузки:

```bash
mkdir -p models/piper-en models/piper-ru

# Английский (по умолчанию)
curl -sL https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/ryan/medium/en_US-ryan-medium.onnx \
  -o models/piper-en/en_US-ryan-medium.onnx
curl -sL https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/ryan/medium/en_US-ryan-medium.onnx.json \
  -o models/piper-en/en_US-ryan-medium.onnx.json

# Русский (по умолчанию)
curl -sL https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/ru/ru_RU/denis/medium/ru_RU-denis-medium.onnx \
  -o models/piper-ru/ru_RU-denis-medium.onnx
curl -sL https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/ru/ru_RU/denis/medium/ru_RU-denis-medium.onnx.json \
  -o models/piper-ru/ru_RU-denis-medium.onnx.json
```

Все доступные голоса: [rhasspy.github.io/piper-samples](https://rhasspy.github.io/piper-samples/).

### 4. Переменные окружения
```bash
cp .env.example .env
```

Отредактируйте `.env`:

**macOS:**
```
DEEPGRAM_API_KEY=ваш_ключ
GROQ_API_KEY=ваш_ключ
ORT_DYLIB_PATH=/opt/homebrew/lib/libonnxruntime.dylib
```

**Windows:**
```powershell
copy .env.example .env
# Отредактируйте .env с вашими API-ключами:
DEEPGRAM_API_KEY=ваш_ключ
GROQ_API_KEY=ваш_ключ
# Путь к DLL ONNX Runtime (отрегулируйте, если установлена в другом месте)
ORT_DYLIB_PATH=C:\ProgramData\chocolatey\lib\onnxruntime\bin\onnxruntime.dll
```

Или установите `ORT_DYLIB_PATH` через переменную окружения в PowerShell:
```powershell
$env:ORT_DYLIB_PATH = "C:\ProgramData\chocolatey\lib\onnxruntime\bin\onnxruntime.dll"
```

### 5. Сборка

```bash
mix deps.get
mix compile    # Компилирует Elixir + Rust (первая сборка занимает несколько минут)
```

### 6. Запуск

```bash
./run.sh
```

Откройте **http://127.0.0.1:5050** в Chrome.

---

## Возможности веб-интерфейса

- **Живой транскрипт** — баблы в стиле чата с оригиналом и переводом
- **29 языков** — переключение языковой пары в настройках, загрузка голосов в один клик
- **Выбор голоса** — несколько голосов на язык с предпрослушиванием
- **Аудио-монитор** — прослушивание переводов в браузере (только Chrome)
- **Start/Stop** — управление движком без перезапуска
- **Mute** — независимое отключение исходящего или входящего потока
- **Закладки** — отметка важных фраз, фильтр по отмеченным
- **Экспорт** — скачать полный транскрипт текстовым файлом
- **Компактный/полный вид** — переключение между подробным и компактным транскриптом
- **Метрики задержки** — для каждой фразы: STT, перевод, TTS и общая задержка
- **Тёмная/светлая тема** — переключение с сохранением

---

## Поддерживаемые языки

| Язык | STT | Перевод | TTS |
|------|-----|---------|-----|
| Английский | + | + | + |
| Арабский | + | + | + |
| Вьетнамский | + | + | + |
| Голландский | + | + | + |
| Греческий | + | + | + |
| Датский | + | + | + |
| Индонезийский | + | + | + |
| Испанский | + | + | + |
| Итальянский | + | + | + |
| Каталанский | + | + | + |
| Китайский | + | + | + |
| Корейский | + | + | — |
| Латышский | + | + | + |
| Немецкий | + | + | + |
| Норвежский | + | + | + |
| Персидский | + | + | + |
| Польский | + | + | + |
| Португальский | + | + | + |
| Румынский | + | + | + |
| Русский | + | + | + |
| Турецкий | + | + | + |
| Украинский | + | + | + |
| Венгерский | + | + | + |
| Финский | + | + | + |
| Французский | + | + | + |
| Хинди | + | + | + |
| Чешский | + | + | + |
| Шведский | + | + | + |
| Японский | + | + | — |

Для TTS нужно скачать голосовую модель Piper для языка (в один клик из веб-интерфейса). Японский и корейский поддерживают STT и перевод, но не имеют голосовой модели Piper.

---

## Решение проблем

**"Engine not starting"**
- Проверьте, что в `.env` указаны валидные API-ключи
- Убедитесь, что `ORT_DYLIB_PATH` указывает на библиотеку onnxruntime
- Запустите `mix compile` для проверки ошибок сборки

**"Нет аудио из звонка"**
- Убедитесь, что BlackHole 16ch настроен в Multi-Output Device
- Проверьте, что приложение для звонков использует BlackHole 2ch как микрофон

**"TTS не работает"**
- Проверьте, что установлен `espeak-ng`: `espeak-ng --version`
- Убедитесь, что файлы голосовых моделей есть в `models/piper-{lang}/`
- Скачайте голоса из Settings в веб-интерфейсе

**"Нет звука в мониторе"**
- Используйте Chrome — Safari не поддерживает маршрутизацию аудио-выхода, необходимую для монитора
- Проверьте, что системный аудио-выход установлен на динамики (не на BlackHole)

**"Ключ Groq показывает invalid"**
- Скорее всего ключ валиден — проверьте кнопкой "Test" в Settings
- Ключи из `.env` работают автоматически, даже если поле в UI пустое

---

## Лицензия

MIT
