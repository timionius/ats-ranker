# ATS Ranker

[![Rust](https://img.shields.io/badge/rust-1.80+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**ATS Ranker** is a CLI tool that automates CV scoring against job descriptions (JDs) harvested from ATS platforms. It finds the best‑matching open positions for a given candidate by combining **semantic embeddings** with a **DeepSeek LLM** evaluator.

The workflow:

1. **Collect** ATS board slugs from a Common Crawl WAT/WARC file.
2. **Refresh** job postings for all known boards (currently only Greenhouse).
3. **Vectorize** all active JDs with a local embedding model.
4. **Rank** JD against a CV – the LLM computes an ATS score, a recruiter score, and detailed feedback.

---

## ✨ Features

- **Multi‑step pipeline** – separate commands for each stage, reusable and scriptable.
- **Local embeddings** – uses `BGE‑small‑en‑v1.5` via `fastembed` – fast, no external API calls.
- **DeepSeek scoring** – each JD is evaluated with your CV; produces structured JSON feedback.
- **DuckDB persistence** – all data (jobs, vectors, scores) stored in a single file.
- **Extensible providers** – the `providers/` module makes it easy to add other ATS platforms (Workday, Lever, etc.).

---

## 🚀 Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (1.80 or later)
- `libssl` and `pkg-config` (on Linux/macOS) – required by some HTTP crates.
  - **Ubuntu/Debian**: `sudo apt install libssl-dev pkg-config`
  - **macOS (Homebrew)**: `brew install openssl pkg-config`

### Installation

```bash
git clone git@github.com:timionius/ats-ranker.git
cd ats-ranker
cargo build --release
```

The binary will be at `target/release/ats-ranker`. You can copy it to your `PATH` or run it directly.

---

## 🔧 Configuration

Set your **DeepSeek API key** as an environment variable:

```bash
export DEEPSEEK_API_KEY="your-api-key-here"
```

For convenience, create a `.env` file in the project root:

```
DEEPSEEK_API_KEY=your-api-key-here
```

The tool reads this key at runtime; it is used **only** during the `rank` command.

---

## 🗄️ Database

The project expects a DuckDB file named **`ats_ranker.duckdb`** in the **project root** (the same directory where you run the commands).  
The path is hard‑coded in the source – you can symlink it if you prefer to store it elsewhere.

> **Note**: The schema is fixed for this version. The database file is provided in the repository; do not modify it manually.

---

## 📦 Usage

```
./target/release/ats-ranker <COMMAND>
```

### 0. Downloading Common Crawl Data (Manual Index Lookup)

The collect-slugs command needs a WARC file from the Common Crawl project. You can find the exact file using the official Common Crawl Index Server, which returns results in JSON Lines format:

1. Open https://index.commoncrawl.org/ in your browser and select one of the CRAWL ARCHIVE.
2. In the search box, enter a URL pattern to find records for Greenhouse boards. For example:

- boards.greenhouse.io
- Or be more specific: url:"boards.greenhouse.io/\*/jobs"

3. The server returns results as JSON Lines (one JSON object per line). Use your browser's File → Save As... menu to save this JSON response to a local file (e.g., CC-MAIN-2026-30-index, according to selected archive).

### 1. `collect-slugs`

Extract Greenhouse board slugs from a Common Crawl **WAT** or **WARC** file.  
Slugs are the subdomain/company identifiers (e.g., `acme` from `acme.greenhouse.io`).

```bash
ats-ranker collect-slugs path/to/CC-MAIN-XXXX-XX-index
```

The slugs are stored in the `boards` table. You only need to run this once per index dump. The output provides exact number of new slugs inserted:

```
ats-ranker % ./target/release/ats-ranker collect-slugs ~/Downloads/CC-MAIN-2026-30-index
Scanned "/Users/user/Downloads/CC-MAIN-2026-30-index": 11845 board URLs matched, 699 new boards inserted, 0 skipped (percent-encoded).
```

---

### 2. `jd-refresh`

Fetch the latest job postings for **all** boards that have been collected.  
Each board is queried via its Greenhouse API endpoint; new jobs are inserted, existing ones are updated.

```bash
ats-ranker jd-refresh
```

After this step, the `jobs` table contains all active openings.

---

### 3. `vectorize`

Compute embeddings for job descriptions or candidate CVs using the local BGE embedding model (no API calls).

#### 3.1. Subcommand: jd

Process all active job descriptions that do not yet have a stored embedding. The resulting vectors are saved in the job_embeddings table. This command is idempotent – already processed jobs are skipped.

```bash
ats-ranker vectorize jd
```

The procedure may take certain amount of time, depending on number of job offers found. Vectors are stored in the `job_embeddings` table. This step is idempotent; already‑embedded jobs are skipped.

#### 3.2. Subcommand: cv

Generate an embedding for a plain text CV file and store it in the database (in the cvs table). This prepares the CV for subsequent ranking without re‑computing the embedding each time.

```bash
ats-ranker vectorize cv ./resume.txt
```

---

### 4. `rank`

Rank top 500 active jobs against a **CV** provided as a plain text file.  
The tool:

- Selects the top‑N most similar JDs using cv ID provided
- Sends prewarm.txt prompt along with the CV as the context for all JDs comparisons
- Sends each JD (title + description) to DeepSeek for detailed scoring.

```bash
ats-ranker rank <.ID from cvs table>
```

Results are written to the `cv_rank` table (see [Output schema](#-output-schema) below).  
The command prints a summary to stdout (e.g., number of jobs processed, any errors).

---

## 📊 Output Schema

All ranking results are stored in the `cv_rank` table:

| Column            | Type      | Description                                               |
| ----------------- | --------- | --------------------------------------------------------- |
| `cv_id`           | INTEGER   | References `cvs(id)` – each run creates a new CV entry.   |
| `job_id`          | BIGINT    | References `jobs(id)`.                                    |
| `ats_score`       | INTEGER   | DeepSeek’s automated ATS compatibility score (0–100).     |
| `recruiter_score` | INTEGER   | Simulated recruiter assessment (0–100).                   |
| `remote_type`     | VARCHAR   | e.g., `"Remote"`, `"Hybrid"`, `"On‑site"`.                |
| `b2b_friendly`    | BOOLEAN   | Whether the company is open to B2B contracts.             |
| `strengths`       | JSON      | List of candidate strengths relative to the JD.           |
| `red_flags`       | JSON      | List of potential mismatches / concerns.                  |
| `summary`         | VARCHAR   | Brief textual evaluation.                                 |
| `scoring_model`   | VARCHAR   | Which LLM was used (e.g., `"deepseek‑chat"`).             |
| `scored_at`       | TIMESTAMP | When the ranking was performed.                           |
| `scoring_error`   | VARCHAR   | If the LLM call failed, the error message is stored here. |

You can query this table with any DuckDB client to generate reports or dashboards.

---

## 🧱 Project Structure

```
ats-ranker/
├── Cargo.toml
├── src/
│   ├── main.rs              # CLI entry point
│   ├── commands/            # Each subcommand
│   │   ├── collect_slugs.rs
│   │   ├── jd_refresh.rs
│   │   ├── vectorize.rs
│   │   └── rank.rs
│   ├── db.rs                # DuckDB connection & queries
│   ├── embeddings.rs        # fastembed wrapper (BGE‑small)
│   ├── deepseek.rs          # DeepSeek API client (scoring)
│   ├── model.rs             # Shared data structures (Job, Board, CV, etc.)
│   ├── utils.rs             # Helpers (text cleaning, similarity)
│   └── providers/           # ATS‑specific fetchers (extensible)
│       ├── mod.rs
│       └── greenhouse.rs    # Greenhouse API implementation
└── prompts/                 # LLM prompt templates
    ├── assessment.txt
    └── prewarm.txt
```

---

## 🔌 Extending the Provider System

To add support for another ATS (e.g., Lever, Workday):

1. Create a new file in `src/providers/` (e.g., `lever.rs`).
2. Implement the `Provider` trait (see `greenhouse.rs` for reference).
3. Register your provider in `providers/mod.rs`.
4. The `jd-refresh` command will automatically iterate over all registered providers.

---

## ⚠️ Troubleshooting

| Issue                                 | Solution                                                                                            |
| ------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `DEEPSEEK_API_KEY` not set            | Export the variable or add it to a `.env` file.                                                     |
| DuckDB file not found                 | Ensure `ats_ranker.duckdb` is in the current working directory.                                     |
| `collect-slugs` fails on WARC parsing | Make sure the file is a valid WAT/WARC from Common Crawl.                                           |
| `vectorize` is slow                   | BGE‑small runs on CPU by default; enable `cargo` features for GPU if needed (see `fastembed` docs). |
| Rate limiting from DeepSeek           | The tool implements exponential backoff; check your API usage limits.                               |

---

## 🤝 Contributing

Issues and pull requests are welcome.  
Please ensure your code passes `cargo fmt` and `cargo clippy`, and add tests when applicable.

---

## 📄 License

This project is licensed under the MIT License – see the [LICENSE](LICENSE) file for details.

```

```
