---
name: yahoo-finance
description: "Look up real-time stock quotes, historical prices, company fundamentals, dividends, splits, and options chains from Yahoo Finance. Use when the user asks about a ticker, a stock price, a company's financials, market data, a portfolio value, or options. Triggers on mentions of stock, ticker, share price, market cap, P/E, dividend, earnings, options, $TSLA-style tickers. No API key required."
license: MIT (see repository LICENSE)
compatibility: "Requires python3 with yfinance installed (bundled in image). Works on Linux."
---

# Yahoo Finance Skill

Quote, fundamentals, and historical market data via the `yfinance` Python package. Anonymous — no API key, no account setup. Rate-limited by Yahoo on their side; be reasonable.

## Current quote

```bash
python3 - <<'PY'
import yfinance as yf, json
t = yf.Ticker("TSLA")
info = t.fast_info
print(json.dumps({
    "symbol": "TSLA",
    "last_price": info.last_price,
    "open": info.open,
    "previous_close": info.previous_close,
    "day_high": info.day_high,
    "day_low": info.day_low,
    "currency": info.currency,
    "market_cap": info.market_cap,
}, indent=2, default=str))
PY
```

Replace `TSLA` with any Yahoo ticker (e.g. `AAPL`, `MSFT`, `^GSPC` for S&P 500, `BTC-USD` for Bitcoin, `EURUSD=X` for FX).

## Historical prices

```bash
python3 - <<'PY'
import yfinance as yf
hist = yf.Ticker("AAPL").history(period="1mo", interval="1d")
print(hist[["Open", "High", "Low", "Close", "Volume"]].to_string())
PY
```

Common `period` values: `1d`, `5d`, `1mo`, `3mo`, `6mo`, `1y`, `2y`, `5y`, `10y`, `ytd`, `max`.
Common `interval` values: `1m`, `5m`, `15m`, `30m`, `60m`, `1h`, `1d`, `1wk`, `1mo`.

For a specific date range use `start="2024-01-01"` and `end="2024-12-31"` instead of `period`.

## Company fundamentals

```bash
python3 - <<'PY'
import yfinance as yf, json
t = yf.Ticker("MSFT")
# .info is a large dict; pick the fields you need
fields = ["shortName", "sector", "industry", "marketCap", "trailingPE",
          "forwardPE", "dividendYield", "beta", "52WeekChange",
          "profitMargins", "revenueGrowth", "earningsGrowth"]
print(json.dumps({k: t.info.get(k) for k in fields}, indent=2, default=str))
PY
```

Quarterly earnings:
```bash
python3 -c "import yfinance as yf; print(yf.Ticker('AAPL').quarterly_earnings)"
```

Financial statements (income statement, balance sheet, cash flow):
```bash
python3 -c "import yfinance as yf; print(yf.Ticker('AAPL').income_stmt.to_string())"
python3 -c "import yfinance as yf; print(yf.Ticker('AAPL').balance_sheet.to_string())"
python3 -c "import yfinance as yf; print(yf.Ticker('AAPL').cashflow.to_string())"
```

## Dividends and splits

```bash
python3 -c "import yfinance as yf; print(yf.Ticker('KO').dividends.tail(10))"
python3 -c "import yfinance as yf; print(yf.Ticker('AAPL').splits)"
```

## Options chain

```bash
python3 - <<'PY'
import yfinance as yf
t = yf.Ticker("SPY")
expirations = t.options
print("Expirations:", expirations[:5])
chain = t.option_chain(expirations[0])
print("\nCalls (first 10):")
print(chain.calls[["strike", "lastPrice", "bid", "ask", "impliedVolatility"]].head(10).to_string())
print("\nPuts (first 10):")
print(chain.puts[["strike", "lastPrice", "bid", "ask", "impliedVolatility"]].head(10).to_string())
PY
```

## Batch quotes

```bash
python3 - <<'PY'
import yfinance as yf, json
data = yf.Tickers("AAPL MSFT GOOGL AMZN NVDA").tickers
quotes = {sym: {
    "price": t.fast_info.last_price,
    "pct": (t.fast_info.last_price / t.fast_info.previous_close - 1) * 100 if t.fast_info.previous_close else None,
} for sym, t in data.items()}
print(json.dumps(quotes, indent=2))
PY
```

## Usage guidance

- Data is Yahoo's — it can be delayed 15-20 minutes for some exchanges, and some fields may be missing for less-liquid tickers.
- Always handle `None` / `NaN` values — they are common for tickers with sparse data.
- For tickers not on US exchanges, use the Yahoo suffix: `SAP.DE` (Frankfurt), `NESN.SW` (Swiss), `7203.T` (Tokyo), `AAPL.L` (London ADR).
- Crypto: use `<SYM>-USD` form (e.g. `BTC-USD`, `ETH-USD`).
- Forex: use `<PAIR>=X` form (e.g. `EURUSD=X`, `GBPJPY=X`).
