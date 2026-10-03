# T8-Lan

**T8-Lan** is een gratis, supercompacte en lichtgewicht netwerk-toolbox voor
installateurs. Pas razendsnel de IP-instellingen van je netwerkkaart aan (statisch ↔
DHCP), scan in één klik alle apparaten op het netwerk, en voer live ping-tests uit -
alles vanuit een klein venstertje in je Windows system tray.

Ontwikkeld door **Turn8.io** - *improving saves time*.

## ⬇️ Download

### ▶︎ [Download T8-Lan voor Windows](https://github.com/turn8io/T8-Lan/releases/download/v0.3.0/T8-Lan-v0.3-setup.exe)

Klik op de link hierboven - de installer (`T8-Lan-v0.3-setup.exe`) wordt **meteen
gedownload**. Dubbelklik en installeer. Klaar - verder hoef je niets met GitHub te doen.
Windows 10/11, ruim 1 MB. Geen account, geen telemetry, geen cloud.

[![Download](https://img.shields.io/github/downloads/turn8io/T8-Lan/total?label=downloads&color=ecc209&cacheSeconds=600)](https://github.com/turn8io/T8-Lan/releases/latest)

---

## Wat kan T8-Lan?

Alles wat een installateur op locatie nodig heeft, in één klein tray-app - hieronder
alle functies op een rij.

### Razendsnel IP-instellingen aanpassen

Kies je netwerkkaart en zie direct het actieve **IP, subnet en gateway**. Schakel met één
klik naar **DHCP** of zet een **statisch IP** - schakelen kan zelfs via het
**rechtermuismenu op het tray-icoon**, zonder het venster te openen.

| DHCP-modus | Statisch IP |
|---|---|
| ![Adapter-instellingen in DHCP-modus](screenshots/Adapter%20Settings%20DHCP.png) | ![Adapter-instellingen met statisch IP](screenshots/Adapter%20Settings%20Static.png) |

- **Ethernet of WiFi** kiezen; bij de eerste start wordt automatisch een actieve adapter gekozen.
- **DHCP**: status is alleen-lezen, klik = direct naar het klembord. **Statisch**: IP,
  subnet en gateway zijn **direct bewerkbaar** (past meteen toe via `netsh`); een pijltje
  wisselt de gateway tussen `.1` en `.254`.
- **Set IP**: typ een adres (Enter = toepassen) of kies uit recente + voorbeeld-IP's.
  Vóór toepassen een **IP-conflict-check** via een ARP-probe (<300 ms).
- **Recente IP's per adapter** (LAN ≠ WiFi gescheiden), met **labels per klant/locatie**.
- **Undo** en **Redo** van de laatste wijziging via de titelbalk.

### Netwerkscan met apparaatherkenning

Eén klik scant het hele lokale netwerk en toont **live** alle bezette IP-adressen, op
numerieke volgorde.

![Netwerkscan met live merk- en apparaatherkenning](screenshots/Netwerk%20Scan.png)

- **Camera's & NVR's**: merk én exact type via **ONVIF WS-Discovery** (vrijwel elk merk),
  **Hikvision SADP**, MAC-OUI (Hikvision, Dahua, Axis, Bosch, Hanwha, Uniview, …) en een
  **NX-fingerprint** op poort 7001 (Network Optix / Nx Witness).
- Twee ARP-rondes per scan, zodat een apparaat dat één broadcast mist alsnog gevonden wordt.
- **ATS-alarmpanelen** (Aritech/Carrier): een lichte identify op TCP **32000** leest
  **type, firmware en paneelnaam** uit - alleen bij apparaten die geen camera/NVR zijn.
- Apparaten met een webinterface krijgen een **geel webicoon** en open je met een klik in
  de browser; per IP is er een knop om het adres te **kopiëren** of direct te **pingen**.

### DHCP-server (nieuw in 0.3)

Voor netwerken zonder DHCP-server, of een apparaat op DHCP dat je met een directe
(cross)kabel aan je laptop hangt. Eén schakelaar zet de gekozen adapter op **192.168.8.8**
(zonder gateway, dus je WiFi-internet blijft werken) en deelt adressen uit vanaf
**192.168.8.100**, standaard een scope van **50**. Server-IP, pool en scope zijn
aanpasbaar; uitgegeven adressen verschijnen live met MAC, hostnaam en vendor, met knoppen
om te pingen of te kopiëren.

- Luistert **alleen op de gekozen adapter**: DHCP-verzoeken via WiFi of een andere kaart
  worden nooit beantwoord.
- Inschakelen gaat altijd via een **waarschuwing**: in een bestaand netwerk legt een
  tweede DHCP-server het netwerk plat.
- Bij uitschakelen (of afsluiten van T8-Lan) krijgt de adapter zijn vorige configuratie
  terug.

### Live ping-test

![Live ping-test met grafiek en statistieken](screenshots/Ping%20Test.png)

Controleer de bereikbaarheid van elk adres met een continue **2 Hz**-ping (via
`IcmpSendEcho`, geen externe processen): **laatste, gemiddelde, min/max en verlies** in
één tegel, met een live grafiek. Vanuit de netwerkscan ping je een gevonden apparaat met
één klik.

### System tray & sneltoetsen

- **Linkerklik** opent het venster; **rechterklik** geeft een contextmenu met `DHCP` en de
  laatst gebruikte statische IP's (per adapter) - schakelen zonder het venster te openen.
- Het tray-icoon draagt een **groene/rode stip** als internet-indicatie (pingt de
  ingestelde DNS, met hysterese tegen flapperen); de **hover-tooltip** toont de modus en
  het huidige IP.
- Globale sneltoetsen vanuit elke app: **Ctrl + Alt + D → DHCP** en
  **Ctrl + Alt + S → laatste statische IP**, met een Windows-toast als bevestiging.

### WiFi-automatisering

- Regels als "verbonden met SSID **X** → schakel naar **DHCP** of statisch IP **Y**".
  T8-Lan pollt de actieve SSID en schakelt automatisch.

### DNS

- **Change DNS** mee-instellen bij een switch; presets **Cloudflare / Google / OpenDNS /
  Quad9 / Custom**, of DNS 1 / DNS 2 zelf invullen.
- Een continue bereikbaarheids-ping toont groen/rood onder de DNS-tab als snelle
  internet-status.

### Uiterlijk & installatie

- Klein venster (opent altijd op 220×300, daarna vrij te vergroten) met een fijne
  bewegende gele rand; het onthoudt zijn positie.
- **NSIS-installer** (per machine). De app draait met admin-rechten en start via een
  **Task Scheduler**-taak automatisch bij login - **zonder telkens een UAC-prompt**, ook op
  accu en zonder tijdslimiet. Uit te zetten via **Hotkeys → Starten met Windows**. Geen
  bureaublad-snelkoppeling, geen telemetry, geen cloud.
- **Eén instantie**: een tweede start brengt het bestaande venster naar voren.
- **Zes talen** (Nederlands, Engels, Duits, Frans, Italiaans, Spaans): volgt standaard de
  Windows-weergavetaal, aan te passen onder **Instellingen**.

---

## Beveiliging

T8-Lan wijzigt Windows-netwerkconfiguratie en heeft daarom admin-rechten nodig.
De broncode is **volledig in te zien** (source-available) zodat te verifiëren is dat er
geen data wordt verzonden buiten je eigen machine.

- Geen telemetry, geen analytics, geen calls naar Turn8.io-servers.
- Settings staan lokaal in `%APPDATA%\io.turn8.t8lan\`.
- Enige uitgaande verbindingen: jouw eigen ping-/DNS-target, de netwerkscan binnen
  je eigen subnet, de DHCP-server (alleen op de gekozen adapter, alleen als je hem zelf
  aanzet) en (eerste install op Win10) de WebView2-bootstrapper.

---

## Licentie

**Proprietair, source-available** - zie [LICENSE](LICENSE). De broncode is zichtbaar
voor transparantie en inspectie; het is **geen open source**. Je mag de officiële,
ongewijzigde T8-Lan-applicatie gratis gebruiken (ook commercieel). Kopiëren, wijzigen,
afgeleide werken maken, code hergebruiken of herdistribueren is niet toegestaan.
Voor ander gebruik of licentievragen: info@turn8.io.

---

[**Turn8.io**](https://www.turn8.io) - *improving saves time*
