# CLAUDE.md - Project Context

## Project Overview
Rust electrical pricing assistant with Claude AI integration. Handles quotations, pricing, stock management via Telegram/WhatsApp.

## Architecture
- **Multi-service async application** using `tokio`
- **Service trait pattern** for all services
- **Error handling** with `thiserror::Error`
- **Configuration** via `config.json`
- **Logging** with `tracing`

## Key Components

### Core Services
- `QueryFulfilment` - Main request handler
- `LLMOrchestrator` - LLM integration with Groq and Claude
- `QuotationService` - Pricing and quotation logic
- `StockService` - Tally ERP integration
- `PriceService` - Metal price fetching

### Data Models
- `Product` enum: `Cable{PowerControl{LT/HT}}`, `Flexible`, etc.
- `Query` enum: `GetQuotation`, `GetPricesOnly`, `GetStock`, `MetalPricing`
- `QuotationRequest`/`QuoteItem` with pricing logic

### Communication
- `TelegramService` - Bot integration
- `WhatsAppService` - Twilio integration
- `PriceAlertService` - Price notifications

## File Structure
```
src/
├── main.rs              # Service orchestration
├── llm/mod.rs          # LLM integration
├── quotation/mod.rs     # Pricing logic
├── prices/
│   ├── mod.rs          # Metal price fetching
│   └── item_prices.rs  # Product definitions
├── stock/mod.rs        # Tally integration
├── communication/      # Telegram/WhatsApp
├── database/mod.rs     # Cost tracking
└── configuration/mod.rs # Config management
```

## Domain Context
- **Electrical components**: Armoured/unarmoured cables, XLPE/PVC insulation, FRLS, LT/HT voltage
- **Pricing**: Base price + discounts + FRLS loading (3%) + PVC loading (5%) + 18% GST
- **Business flow**: Query → Parse → Price lookup → Generate quotation/response

## Integration Points
- **Claude API** for query understanding through tool use
- **Groq API** for query understanding through tool use
- **Tally ERP** for stock queries via XML
- **MCX websites** for metal prices
- **Telegram/WhatsApp** for user communication

## Key Patterns
- Services implement `Service` trait
- Async error propagation with `?` operator
- Configuration via structs with `serde`
- Price calculations with f32 precision
- JSON deserialization for Claude responses