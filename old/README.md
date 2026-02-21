# Chia Carbon Credit Market Making Scripts

This repository contains Python scripts for automated market making of carbon credit CATs (Chia Asset Tokens) on the Chia blockchain, integrated with the Dexie exchange and Splash platform.

## Overview

The system consists of three main components:

1. **Market Maker** (`splash_carbon_market_maker_xch.py`) - Monitors liquidity and creates offers when needed for both XCH and USDC pairs
2. **Unified Offer Creator** (`make_offer.py`) - Creates offers against XCH (dynamic pricing) or bridged USDC (fixed pricing)
3. **Common Module** (`common.py`) - Shared functionality across all scripts

### Legacy Scripts (Deprecated)
- `make_offer_for_xch.py` - XCH-specific offer creator (use `make_offer.py --pair xch` instead)
- `make_offer_for_b_usdc.py` - USDC-specific offer creator (use `make_offer.py --pair usdc` instead)

## Features

- **🔄 Automated Market Making**: Monitors market depth and automatically creates offers when liquidity falls below thresholds
- **💰 Multi-Pair Support**: Supports both XCH and bridged USDC trading pairs
- **📈 Dynamic Pricing**: Fetches real-time XCH prices for accurate USD-based pricing
- **🎯 Configurable Targets**: Customizable liquidity thresholds for different offer sizes
- **🔍 Dry Run Mode**: Preview actions before execution
- **⚡ Smart Expiry**: Pair-specific expiry times (minutes for XCH, hours for USDC)
- **🛡️ Robust Error Handling**: Comprehensive validation and error reporting
- **📊 Enhanced Status Display**: Clear visual feedback with progress indicators

## Requirements

- Python 3.8+
- Chia blockchain node running locally
- Access to Dexie API
- Required Python packages (see `requirements.txt`)

## Installation

1. Clone the repository:
```bash
git clone <repository-url>
cd chia-carbon-market-maker
```

2. Install dependencies:
```bash
pip install -r requirements.txt
```

3. Update the `CHIA_BIN_PATH` in `common.py` to match your Chia installation:
```python
CHIA_BIN_PATH = "/your/path/to/chia"
```

4. Make scripts executable:
```bash
chmod +x *.py
```

## Quick Start

### Basic Market Making
```bash
# XCH pair with default settings
python splash_carbon_market_maker_xch.py

# USDC pair 
python splash_carbon_market_maker_xch.py --pair usdc

# Preview actions without executing
python splash_carbon_market_maker_xch.py --dry-run
```

### Manual Offer Creation
```bash
# Create XCH offers
python make_offer.py --offer-wallet <CAT_ID> --pair xch --count 10 --repeat 5

# Create USDC offers
python make_offer.py --offer-wallet <CAT_ID> --pair usdc --count 1 --expiry-hours 48
```

## Usage

### Market Making

The market maker monitors liquidity and maintains target offer counts:

```bash
# Basic usage with defaults
python splash_carbon_market_maker_xch.py --cat <CAT_ID>

# Custom pair and thresholds
python splash_carbon_market_maker_xch.py \
    --cat <CAT_ID> \
    --pair usdc \
    --ones-target 10 \
    --tens-target 5 \
    --hundreds-target 2

# Dry run to see what would happen
python splash_carbon_market_maker_xch.py \
    --cat <CAT_ID> \
    --dry-run
```

**Market Maker Options:**
- `--cat`: Asset ID of the CAT being market made
- `--pair`: Trading pair (`xch` or `usdc`)
- `--ones-target`: Target number of 1-unit offers (default: 5)
- `--tens-target`: Target number of 10-unit offers (default: 2) 
- `--hundreds-target`: Target number of 100-unit offers (default: 1)
- `--dry-run`: Preview actions without creating offers
- `--make-offer-path`: Path to make_offer.py script

### Unified Offer Creation

Create individual offers with the unified script:

```bash
# XCH offers with dynamic pricing
python make_offer.py \
    --offer-wallet <CAT_ID> \
    --pair xch \
    --count 10 \
    --expiry-minutes 60 \
    --repeat 5

# USDC offers with fixed pricing
python make_offer.py \
    --offer-wallet <CAT_ID> \
    --pair usdc \
    --count 1 \
    --expiry-hours 24 \
    --repeat 3 \
    --cancel-after-create
```

**Unified Offer Options:**
- `--offer-wallet`: Asset ID of the CAT being offered (required)
- `--pair`: Trading pair (`xch` or `usdc`, default: `xch`)
- `--accept-wallet`: Override default accept wallet (auto-detected)
- `--count`: Number of CATs to offer (default: 1)
- `--expiry-minutes`: Expire in minutes (overrides --expiry-hours)
- `--expiry-hours`: Expire in hours 
- `--repeat`: Create multiple identical offers (default: 1)
- `--cancel-after-create`: Cancel offer after creation (for market making)

## Configuration

### Carbon Credit Pricing

Update the `CARBON_CREDIT_PRICES` dictionary in `common.py`:

```python
CARBON_CREDIT_PRICES = {
    # Agricultural Reforestation Project 2022
    "4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7": 7.75,
    # Agricultural Reforestation Project 2020  
    "e257aca547a83020e537e87f8c83e9332d2c3adb729c052e6f04971317084327": 7.50,
    # Add your carbon credit projects here
    "your_cat_id_here": 8.00,
}
```

### Market Depth Targets

Modify targets in the market maker script or use command-line options:

```python
# Default targets
ONES_TARGET = 5      # 1-unit offers
TENS_TARGET = 2      # 10-unit offers  
HUNDREDS_TARGET = 1  # 100-unit offers
```

### Expiry Configuration

Adjust expiry times in the market maker:

```python
PAIR_EXPIRY_CONFIG = {
    "xch": {"unit": "minutes", "value": 65},   # XCH offers expire in 65 minutes
    "usdc": {"unit": "hours", "value": 24},    # USDC offers expire in 24 hours
}
```

## Automation

### Cron Job Examples

```bash
# Market make XCH every 10 minutes
*/10 * * * * cd /path/to/scripts && python splash_carbon_market_maker_xch.py >> xch_market.log 2>&1

# Market make USDC every hour
0 * * * * cd /path/to/scripts && python splash_carbon_market_maker_xch.py --pair usdc >> usdc_market.log 2>&1

# Multiple CATs with custom targets
*/15 * * * * cd /path/to/scripts && python splash_carbon_market_maker_xch.py --cat CAT_ID_1 --ones-target 10 >> market1.log 2>&1
*/15 * * * * cd /path/to/scripts && python splash_carbon_market_maker_xch.py --cat CAT_ID_2 --pair usdc >> market2.log 2>&1
```

### Systemd Service Example

Create `/etc/systemd/system/carbon-market-maker.service`:

```ini
[Unit]
Description=Carbon Credit Market Maker
After=network.target

[Service]
Type=simple
User=chia
WorkingDirectory=/path/to/scripts
ExecStart=/usr/bin/python3 splash_carbon_market_maker_xch.py
Restart=always
RestartSec=600

[Install]
WantedBy=multi-user.target
```

## Code Quality

The code follows Python best practices and passes common linting tools:

### Install Development Tools
```bash
pip install black isort flake8 mypy pylint
```

### Run Quality Checks
```bash
# Format and check code
black *.py
isort *.py
flake8 *.py
mypy *.py
pylint *.py
```

### Pre-commit Setup
```bash
# Install pre-commit
pip install pre-commit

# Set up git hooks (optional)
pre-commit install
```

## Architecture

### Common Module (`common.py`)
Provides shared functionality:
- Chia RPC communication and offer creation
- Price fetching (XCH rates and carbon credit pricing)
- API communication with Splash platform
- Input validation and utility functions
- Currency conversion and formatting

### Market Maker (`splash_carbon_market_maker_xch.py`)
1. Fetches current market depth from Dexie API
2. Compares against configurable target thresholds
3. Creates missing offers using the unified offer script
4. Provides detailed status reporting and dry-run capability
5. Supports both XCH and USDC pairs

### Unified Offer Creator (`make_offer.py`)
1. Supports both XCH and USDC trading pairs
2. Calculates pricing (dynamic for XCH, fixed for USDC)  
3. Creates Chia blockchain offers via RPC
4. Optionally cancels offers (for market making strategies)
5. Posts offers to Splash platform

## Monitoring and Logging

### Log Analysis
```bash
# Monitor market maker activity
tail -f market_maker.log

# Check for errors
grep -i error *.log

# Monitor offer creation
grep -i "offer created" *.log
```

### Status Monitoring
The market maker provides detailed status output:
```
Market Making Status for XCH Pair
============================================
Asset ID: 4a168910b533e6bb9ddf82a776f8d6248308abd3d56b6f4423a3e1de88f466e7
Current time: 2025-01-28T10:30:00
Pair: XCH

Market Depth Analysis:
  1-unit offers:    3 (target:  5) ⚠
  10-unit offers:   2 (target:  2) ✓
  100-unit offers:  0 (target:  1) ⚠
```

## Troubleshooting

### Common Issues

**Chia Node Connection:**
```bash
# Check if Chia is running
chia show -s

# Test RPC connectivity
chia rpc wallet get_wallets
```

**API Connectivity:**
```bash
# Test Dexie API
curl "https://api.dexie.space/v1/offers?offered=CAT_ID&requested=xch"

# Check Splash connectivity
curl -X POST http://your-splash-server:4000 -d '{"test": "connection"}'
```

**Permission Issues:**
```bash
# Make scripts executable
chmod +x *.py

# Check file permissions
ls -la *.py
```

### Debug Mode

Enable verbose output for debugging:
```bash
# Run with debug output
python -v splash_carbon_market_maker_xch.py --dry-run

# Check specific offer creation
python make_offer.py --offer-wallet CAT_ID --pair xch --count 1 --dry-run
```

## Migration from Legacy Scripts

If you're upgrading from the original scripts:

### From `make_offer_for_xch.py`:
```bash
# Old way
python make_offer_for_xch.py --offer-wallet CAT_ID --count 10

# New way  
python make_offer.py --offer-wallet CAT_ID --pair xch --count 10
```

### From `make_offer_for_b.usdc.py`:
```bash
# Old way
python make_offer_for_b.usdc.py --offer-wallet CAT_ID --count 1

# New way
python make_offer.py --offer-wallet CAT_ID --pair usdc --count 1
```

### Market Maker Updates:
- Now supports both XCH and USDC pairs
- Enhanced status display and dry-run mode
- Configurable targets via command line
- Better error handling and logging

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Follow the existing code style (Black formatting)
4. Add type hints to new functions
5. Update tests and documentation
6. Ensure all linting passes (`black *.py && flake8 *.py && mypy *.py`)
7. Commit your changes (`git commit -m 'Add amazing feature'`)
8. Push to the branch (`git push origin feature/amazing-feature`)
9. Open a Pull Request

## License

[Add your license here]

## Support

For issues or questions:

1. **Check the logs** for detailed error information
2. **Ensure your Chia node** is running and fully synced
3. **Verify network connectivity** to Dexie and Splash APIs
4. **Review configuration** parameters in `common.py`
5. **Use dry-run mode** to debug market making logic
6. **Check file permissions** and script paths

### Getting Help

- Create an issue in the repository
- Include relevant log output
- Specify your Python and Chia versions
- Describe your configuration and environment

---

## Changelog

### v2.0.0 (Latest)
- ✨ Unified offer creation script supporting both XCH and USDC
- 🎯 Enhanced market maker with configurable targets and dry-run mode  
- 📊 Improved status display with visual indicators
- 🔧 Better error handling and validation
- 📚 Comprehensive documentation and examples

### v1.0.0 (Legacy)
- Basic market making for XCH pairs
- Separate scripts for XCH and USDC offers
- Core functionality for carbon credit trading

