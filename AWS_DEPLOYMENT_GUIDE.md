# AWS Deployment Guide - Polymarket Trading Bot

Komplette Anleitung für Laien: Bot auf AWS deployen für minimale Latenz

---

## Warum AWS?

- **~10-20ms Latenz** zu Polymarket (statt 100-200ms von zuhause)
- **24/7 verfügbar** ohne deinen PC laufen zu lassen
- **Stabile Verbindung** ohne Internet-Ausfälle
- **Skalierbar** - mehr CPU Power wenn nötig

---

## Teil 1: AWS Account erstellen

### 1.1 Account registrieren
1. Gehe zu: https://aws.amazon.com
2. Klicke auf "Create an AWS Account"
3. Gib deine Email ein und erstelle ein Passwort
4. Folge den Schritten:
   - Kontaktinformationen eingeben
   - Kreditkarte hinzufügen (für Verifizierung, aber wir bleiben im Free Tier!)
   - Telefonnummer verifizieren
5. Wähle den **"Basic Support - Free"** Plan

**Kosten:** Die ersten 12 Monate sind die meisten Services kostenlos (Free Tier)

---

## Teil 2: EC2 Instance erstellen

### 2.1 Region auswählen
1. Nach dem Login, oben rechts siehst du die **Region** (z.B. "N. Virginia")
2. Klicke darauf und wähle: **"US East (N. Virginia) us-east-1"**
   - ⚠️ **WICHTIG:** Diese Region ist am nächsten zu Polymarket's Servern!

### 2.2 EC2 Dashboard öffnen
1. Oben links auf "Services" klicken
2. "EC2" suchen und anklicken
3. Linke Sidebar: "Instances" anklicken

### 2.3 Instance starten
1. Klicke den orangen Button: **"Launch Instance"**
2. **Name:** `polymarket-trading-bot`
3. **Application and OS Image (AMI):**
   - Wähle: **"Ubuntu Server 24.04 LTS"**
   - Wichtig: Free tier eligible!

4. **Instance type:**
   - Wähle: **"t2.micro"** (Free tier eligible)
   - Das reicht völlig für den Bot!

5. **Key pair (login):**
   - Klicke "Create new key pair"
   - Name: `polymarket-bot-key`
   - Key pair type: **RSA**
   - Private key file format: **`.pem`** (für Windows mit PuTTY: `.ppk`)
   - Klicke "Create key pair"
   - ⚠️ **WICHTIG:** Die Datei wird heruntergeladen - **NICHT VERLIEREN!**
   - Speichere sie z.B. in: `C:\Users\giedz\aws-keys\polymarket-bot-key.pem`

6. **Network settings:**
   - ✅ Allow SSH traffic from: **Anywhere** (0.0.0.0/0)
   - (Du kannst später nur deine IP erlauben für mehr Sicherheit)

7. **Configure storage:**
   - **30 GiB** gp3 (Free tier gibt 30GB)
   - Das ist mehr als genug!

8. Klicke unten rechts: **"Launch instance"**

9. Warte 1-2 Minuten bis Status "Running" ist (grün)

---

## Teil 3: Mit dem Server verbinden

### 3.1 Windows - SSH Setup

#### Option A: PowerShell (einfachste Methode)

1. Öffne PowerShell (Windows-Taste, tippe "PowerShell")

2. Navigiere zum Ordner mit deinem Key:
```powershell
cd C:\Users\giedz\aws-keys
```

3. Setze Berechtigungen für den Key (nur beim ersten Mal):
```powershell
icacls polymarket-bot-key.pem /inheritance:r
icacls polymarket-bot-key.pem /grant:r "$($env:USERNAME):(R)"
```

4. Kopiere die **Public IPv4 address** deiner EC2 Instance aus dem AWS Dashboard
   - Sollte aussehen wie: `54.123.45.67`

5. Verbinde dich:
```powershell
ssh -i polymarket-bot-key.pem ubuntu@54.123.45.67
```
(Ersetze `54.123.45.67` mit deiner IP!)

6. Bei der Frage "Are you sure...?" tippe `yes` und Enter

✅ Du bist jetzt auf deinem AWS Server!

#### Option B: PuTTY (falls PowerShell nicht funktioniert)

1. Downloade PuTTY: https://www.putty.org/
2. Öffne PuTTYgen (kommt mit PuTTY)
3. Load → Wähle deine `.pem` Datei
4. "Save private key" → Speichere als `.ppk`
5. Öffne PuTTY:
   - Host Name: `ubuntu@54.123.45.67` (deine IP)
   - Port: 22
   - Connection → SSH → Auth → Credentials → Private key file: Wähle deine `.ppk` Datei
   - Klicke "Open"

---

## Teil 4: Server einrichten

### 4.1 System updaten
```bash
sudo apt update && sudo apt upgrade -y
```
(Dauert ~2-3 Minuten)

### 4.2 Rust installieren
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
- Bei der Frage wähle Option `1` (default installation)
- Danach:
```bash
source $HOME/.cargo/env
```

Teste ob Rust installiert ist:
```bash
rustc --version
```
Sollte zeigen: `rustc 1.xx.x`

### 4.3 Git installieren
```bash
sudo apt install git -y
```

---

## Teil 5: Bot deployen

### 5.1 Repository clonen
```bash
cd ~
git clone https://github.com/Maciej-web/Polymarket-Kalshi-Arbitrage-bot.git
cd Polymarket-Kalshi-Arbitrage-bot
```

### 5.2 Branch wechseln
```bash
git checkout claude/multi-market-trading-bot-JRod3
```

### 5.3 .env Datei erstellen
```bash
nano .env
```

Füge ein (ersetze mit deinen echten Werten!):
```env
# Polymarket Credentials
POLY_PRIVATE_KEY=dein_private_key_hier
POLY_API_KEY=dein_api_key_hier
POLY_API_SECRET=dein_api_secret_hier
POLY_API_PASSPHRASE=dein_passphrase_hier

# Trading Settings
DRY_RUN=1                    # 1 = Dry Run, 0 = Echtes Trading
EXECUTION_THRESHOLD_CENTS=100

# Optional: Nur bestimmte Kategorien
# ENABLED_CATEGORIES=crypto,sports,politics,business
```

**Speichern:**
- Drücke `Ctrl+X`
- Drücke `Y` (Yes)
- Drücke `Enter`

⚠️ **WICHTIG:** Deine Private Keys NIE mit jemandem teilen!

### 5.4 Bot kompilieren (dauert ~5 Minuten beim ersten Mal)
```bash
cargo build --release
```

Warte bis du siehst: `Finished release profile`

---

## Teil 6: Bot starten

### 6.1 Dry-Run Test
```bash
./target/release/prediction-market-arbitrage
```

Du solltest sehen:
```
🚀 Polymarket Multi-Market Trading System v3.0
📊 Market discovery complete: ... markets found
✅ All systems operational
```

**Test erfolgreich!** Drücke `Ctrl+C` um zu stoppen.

### 6.2 Bot im Hintergrund laufen lassen

Wir nutzen `screen` damit der Bot auch nach dem Logout weiterläuft:

```bash
sudo apt install screen -y
screen -S trading-bot
```

Jetzt bist du in einer neuen "Screen Session". Starte den Bot:
```bash
cd ~/Polymarket-Kalshi-Arbitrage-bot
./target/release/prediction-market-arbitrage
```

**Bot läuft jetzt!**

**Screen verlassen (Bot läuft weiter):**
- Drücke: `Ctrl+A` dann `D` (detach)

**Zurück zum Bot (Logs anschauen):**
```bash
screen -r trading-bot
```

**Bot stoppen:**
- In der Screen Session: `Ctrl+C`

**Screen Liste anzeigen:**
```bash
screen -ls
```

---

## Teil 7: Bot updaten & Konfiguration ändern

### 7.1 Code Updates installieren

Wenn neue Updates verfügbar sind:

```bash
# 1. Screen Session öffnen
screen -r trading-bot

# 2. Bot stoppen: Ctrl+C drücken

# 3. Updates holen
cd ~/Polymarket-Kalshi-Arbitrage-bot
git pull origin claude/multi-market-trading-bot-JRod3

# 4. Neu kompilieren (dauert ~1-2 Minuten)
cargo build --release

# 5. Bot neu starten
./target/release/prediction-market-arbitrage

# 6. Screen verlassen: Ctrl+A dann D
```

### 7.2 Kapital-Limits einstellen

Je nachdem wieviel Kapital du einsetzen willst, musst du verschiedene Parameter anpassen.

#### Für $100 Kapital (Anfänger - EMPFOHLEN)
```bash
# Bot stoppen (Ctrl+C)
nano .env
```

Füge hinzu oder ändere:
```env
# Circuit Breaker Limits für $100 Kapital
MAX_DAILY_LOSS_USD=25              # Max $25 Verlust pro Tag (25%)
MAX_POSITION_SIZE_USD=10           # Max $10 pro Trade (10%)
EXECUTION_THRESHOLD_CENTS=50       # Min 0.5% Profit (50 cents bei $100)
```

Speichern: `Ctrl+X`, `Y`, `Enter`

#### Für $1,000 Kapital (Fortgeschritten)
```bash
nano .env
```

```env
# Circuit Breaker Limits für $1,000 Kapital
MAX_DAILY_LOSS_USD=100             # Max $100 Verlust pro Tag (10%)
MAX_POSITION_SIZE_USD=50           # Max $50 pro Trade (5%)
EXECUTION_THRESHOLD_CENTS=100      # Min 1% Profit (100 cents bei $100)
```

#### Für $10,000 Kapital (Profi)
```bash
nano .env
```

```env
# Circuit Breaker Limits für $10,000 Kapital
MAX_DAILY_LOSS_USD=500             # Max $500 Verlust pro Tag (5%)
MAX_POSITION_SIZE_USD=200          # Max $200 pro Trade (2%)
EXECUTION_THRESHOLD_CENTS=100      # Min 1% Profit
```

**Wichtige Erklärungen:**

- **MAX_DAILY_LOSS_USD**: Bot stoppt automatisch nach diesem Verlust
- **MAX_POSITION_SIZE_USD**: Max Betrag pro einzelnem Trade
- **EXECUTION_THRESHOLD_CENTS**: Minimum Profit in Cents bei $100 Position
  - 50 = 0.5% Mindestprofit
  - 100 = 1.0% Mindestprofit
  - 200 = 2.0% Mindestprofit

**Defaults (wenn nicht gesetzt):**
- MAX_DAILY_LOSS_USD = $500
- MAX_POSITION_SIZE_USD = $100
- EXECUTION_THRESHOLD_CENTS = $100 (aus ARB_THRESHOLD)

### 7.3 Andere wichtige Parameter

```bash
nano .env
```

**Kategorien aktivieren/deaktivieren:**
```env
# Nur Crypto und Sports handeln
ENABLED_CATEGORIES=crypto,sports

# Alle Kategorien (default wenn nicht gesetzt)
ENABLED_CATEGORIES=crypto,sports,politics,business
```

**Anzahl Markets pro Kategorie ändern:**

Das geht nur im Code:
```bash
nano src/config.rs
```

Ändere Zeile 22:
```rust
pub const MARKETS_PER_CATEGORY: usize = 100;  // 100 = 400 total (4 Kategorien)
```

Mögliche Werte:
- `50` = 200 total markets (weniger Last)
- `100` = 400 total markets (EMPFOHLEN)
- `150` = 600 total markets (mehr CPU needed)

Nach Änderung neu kompilieren:
```bash
cargo build --release
./target/release/prediction-market-arbitrage
```

### 7.4 Vollständige .env Vorlage

```env
# ============================================
# POLYMARKET CREDENTIALS (PFLICHT)
# ============================================
POLY_PRIVATE_KEY=dein_private_key_hier
POLY_FUNDER=dein_wallet_address_hier

# ============================================
# TRADING MODE (PFLICHT)
# ============================================
DRY_RUN=1                          # 1 = Dry Run (Test), 0 = Live Trading

# ============================================
# CIRCUIT BREAKER (Optional - hat Defaults)
# ============================================
MAX_DAILY_LOSS_USD=500             # Max Verlust pro Tag bevor Stop
MAX_POSITION_SIZE_USD=100          # Max Betrag pro Trade

# ============================================
# PROFIT THRESHOLD (Optional)
# ============================================
EXECUTION_THRESHOLD_CENTS=100      # Min Profit in Cents bei $100 Position

# ============================================
# MARKET KATEGORIEN (Optional - default: alle)
# ============================================
# Nur bestimmte Kategorien handeln:
# ENABLED_CATEGORIES=crypto,sports,politics,business

# ============================================
# DEBUGGING (Optional)
# ============================================
# PRICE_LOGGING=1                  # Alle Preis-Updates loggen (viel Output!)
# TEST_ARB=1                       # Test-Arbitrage injizieren
```

### 7.5 Config-Änderungen anwenden

**Nach .env Änderung:**
```bash
# Bot einfach neu starten
# In Screen: Ctrl+C
./target/release/prediction-market-arbitrage
# Ctrl+A dann D
```

**Nach Code-Änderung (z.B. MARKETS_PER_CATEGORY):**
```bash
# Neu kompilieren
cargo build --release
# Starten
./target/release/prediction-market-arbitrage
```

---

## Teil 8: Monitoring

### 8.1 Logs anschauen (live)
```bash
screen -r trading-bot
```

### 8.2 Systemstatus checken
```bash
# CPU/Memory Usage
htop
```
(Installieren: `sudo apt install htop -y`)

### 8.3 Disk Space checken
```bash
df -h
```

---

## Teil 9: Wichtige Befehle

### Screen Management
```bash
screen -S trading-bot           # Neue Session erstellen
screen -r trading-bot           # Session wieder öffnen
screen -ls                      # Alle Sessions anzeigen
# In Session: Ctrl+A dann D     # Session verlassen (Bot läuft weiter)
```

### Bot Management
```bash
cd ~/Polymarket-Kalshi-Arbitrage-bot
git pull origin claude/multi-market-trading-bot-JRod3    # Updates holen
cargo build --release                                     # Neu kompilieren
./target/release/prediction-market-arbitrage              # Starten
```

### System Befehle
```bash
htop                # System Monitor
df -h              # Disk Usage
free -h            # RAM Usage
sudo reboot        # Server neu starten
exit               # SSH Verbindung trennen
```

---

## Teil 10: Sicherheit (WICHTIG!)

### 10.1 Firewall einrichten
```bash
sudo apt install ufw -y
sudo ufw allow 22/tcp
sudo ufw enable
```

### 10.2 Nur deine IP für SSH erlauben

1. Finde deine IP: https://whatismyipaddress.com/
2. In AWS Dashboard → EC2 → Instances → Deine Instance auswählen
3. Unten: "Security" Tab → Security Group anklicken
4. "Inbound rules" → "Edit inbound rules"
5. Bei SSH Regel: Ändere "Anywhere" zu "My IP"
6. Deine IP wird automatisch eingetragen
7. Save

### 10.3 Automatisches Backup der .env
```bash
# Backup erstellen
cp .env .env.backup

# Bei Bedarf wiederherstellen
cp .env.backup .env
```

---

## Teil 11: Kosten

### Free Tier (12 Monate kostenlos):
- ✅ t2.micro Instance: 750 Stunden/Monat (= 24/7!)
- ✅ 30GB Storage
- ✅ 15GB Datenübertragung

### Nach Free Tier (ca. Kosten):
- t2.micro: ~$8-10/Monat
- 30GB Storage: ~$3/Monat
- Datenübertragung: ~$1-2/Monat

**Total:** ~$12-15/Monat nach dem ersten Jahr

### Kosten sparen:
- Stopped Instance = nur Storage-Kosten (~$3/Monat)
- Bot nur nachts laufen lassen

---

## Teil 12: Troubleshooting

### Problem: "Connection refused"
```bash
# Überprüfe ob Instance läuft
# In AWS Dashboard: Instance State sollte "Running" sein
```

### Problem: "Permission denied (publickey)"
```bash
# Key Berechtigungen prüfen
icacls polymarket-bot-key.pem
# Sollte nur deinen User zeigen
```

### Problem: Bot stürzt ab
```bash
# Logs anschauen
screen -r trading-bot
# Oder letzten Absturz:
dmesg | tail -50
```

### Problem: "Disk full"
```bash
# Rust Cache aufräumen
cargo clean
# Alte Logs löschen
rm -f *.log
```

### Problem: Bot findet keine Markets
```bash
# .env checken
cat .env
# Internetverbindung testen
ping -c 3 gamma-api.polymarket.com
```

---

## Teil 13: Vom echten Trading (DRY_RUN=0)

⚠️ **NUR WENN DU BEREIT BIST!**

1. Stoppe den Bot (`Ctrl+C`)
2. Editiere .env:
```bash
nano .env
```
3. Ändere: `DRY_RUN=0`
4. Speichere: `Ctrl+X`, `Y`, `Enter`
5. Starte Bot:
```bash
./target/release/prediction-market-arbitrage
```

**Der Bot traded jetzt mit echtem Geld!**

**Circuit Breaker schützt dich:**
- Max $500 Daily Loss (konfigurierbar in Code)
- Max Position Size per Market
- Automatischer Stop bei Fehlern

---

## Schnell-Referenz

### SSH Verbinden
```powershell
ssh -i C:\Users\giedz\aws-keys\polymarket-bot-key.pem ubuntu@DEINE_IP
```

### Bot starten
```bash
screen -S trading-bot
cd ~/Polymarket-Kalshi-Arbitrage-bot
./target/release/prediction-market-arbitrage
# Ctrl+A dann D zum verlassen
```

### Bot Logs checken
```bash
screen -r trading-bot
```

### Bot updaten
```bash
# Screen öffnen
screen -r trading-bot
# Bot stoppen: Ctrl+C
cd ~/Polymarket-Kalshi-Arbitrage-bot
git pull origin claude/multi-market-trading-bot-JRod3
cargo build --release
./target/release/prediction-market-arbitrage
# Verlassen: Ctrl+A dann D
```

### Kapital-Limits Übersicht

| Kapital | MAX_DAILY_LOSS_USD | MAX_POSITION_SIZE_USD | EXECUTION_THRESHOLD_CENTS |
|---------|--------------------|-----------------------|---------------------------|
| $100    | $25 (25%)          | $10 (10%)             | 50 (0.5% profit)          |
| $1,000  | $100 (10%)         | $50 (5%)              | 100 (1.0% profit)         |
| $10,000 | $500 (5%)          | $200 (2%)             | 100 (1.0% profit)         |

In `.env` einstellen:
```bash
nano .env
# Parameter hinzufügen, speichern mit Ctrl+X, Y, Enter
# Bot neu starten
```

---

## Support & Fragen

Bei Problemen:
1. Checke die Logs: `screen -r trading-bot`
2. Teste Internetverbindung: `ping google.com`
3. Checke System Resources: `htop`
4. Guck in Teil 12 (Troubleshooting)

---

**Du bist jetzt bereit! 🚀**

Dein Bot läuft auf AWS mit minimaler Latenz zu Polymarket!
