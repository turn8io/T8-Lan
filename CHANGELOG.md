# Wijzigingen per versie

Nieuwste bovenaan. De datum is de publicatiedatum van de release op GitHub; de installer
heet steeds `T8-Lan-v<major.minor>-setup.exe`. Bijwerken hoort bij elke release, in dezelfde
commit (zie `CLAUDE.md`).

## Niet uitgebracht

- Repo: GitHub Actions-build verwijderd; releases worden lokaal gebouwd en ondertekend.
- Repo: dode scripts (`scripts\rename-installer.ps1`, `scripts\sign.ps1`) en het npm-script
  `release` verwijderd (vervangen door `release:signed`); ongebruikte vertaalsleutel
  `dns.custom` weg; `download-stats.ps1` wijst naar de juiste repo; `CLAUDE.md` en dit
  bestand toegevoegd.

## 0.5.0 - 2026-10-26

### Nieuw
- Hotspot op een **gekozen WiFi-kaart**: Automatisch of een van de fysieke kaarten. De app
  kiest zelf de techniek (Wi-Fi Direct of hosted network) en controleert aan het MAC-adres of
  de hotspot echt op die kaart staat; anders probeert hij de andere techniek of geeft één
  duidelijke melding.
- **Internet delen** via de hotspot (vinkje, standaard uit): een WinNAT-regel voor het
  hotspot-subnet, ook live aan en uit te zetten. Geen ICS, geen 192.168.137.x; het blijft
  192.168.8.x. De DHCP-server deelt altijd de DNS-voorkeur uit de DNS-tab uit.
- **DHCP-server optioneel** op de hotspot: standaard aan, uit te zetten voor apparaten met een
  vast IP. De hotspot houdt dan zelf 192.168.8.8.

### Gewijzigd
- Standaardwachtwoord van de hotspot is `T8online`; een opgeslagen oude standaard
  (`turn8-lan`) gaat automatisch mee.
- Netwerkkaart-dropdown: zo breed als het venster, verbindingsstip per kaart, verbonden
  kaarten bovenaan.
- DNS-gezondheid kleurt het wereldbol-icoon groen of rood in plaats van een streep eronder.
- Turn8-logo met het echte verloop (SVG en PNG).

### Opgelost
- De netwerkkaart-dropdown sloot zodra je erin scrolde.
- De hotspot startte wel, maar werd afgebroken op kaarten waarvan de virtuele adapter de naam
  van de fysieke kaart erft (o.a. Qualcomm FastConnect 7800): "virtuele hotspot-adapter niet
  gevonden".
- Restanten na een crash of hard afsluiten (server-IP, hosted network, NAT) worden bij de
  volgende start opgeruimd; bij stoppen geeft de hotspot alles weer vrij.

### Beveiliging
- Afhankelijkheden met een bekend lek bijgewerkt, alleen naar versies van minstens 7 dagen
  oud: vite 6.4.4, postcss 8.5.31, nanoid 3.3.20, browserslist 4.29.4,
  baseline-browser-mapping 2.11.29, source-map-js 1.2.2, @babel/core 7.29.7, tokio 1.53.3,
  anyhow 1.0.103, serde_with 3.21.0, plist 1.10.1, notify-rust 4.18.2 (quick-xml 0.42.0).
  `.npmrc` dwingt die wachttijd af (`min-release-age=7`).

## 0.4.0 - 2026-10-03

- Nieuw: **WiFi-hotspot** met de eigen DHCP-server erachter (Wi-Fi Direct; eigen netwerknaam en
  wachtwoord; zelfde 192.168.8.8 en pool als bij de kabel; geen internet-delen).
- Installer en uninstaller met het Turn8-logo in de kopbalk en het T8-Lan-icoon.
- Tray: oranje stip bij een wisselvallige verbinding.
- Eerste **ondertekende** release (Azure Trusted Signing, Turn8.io BV) via
  `scripts/release-signed.ps1`.
- Opgelost: keuzevelden staken bij lange teksten (NL, DE) buiten de tegels; het achtste
  tab-icoon viel buiten beeld (venster opent op 240 pixels breed); DHCP-tab zonder infoteksten.

## 0.3.2 - 2026-10-03

- WiFi-hotspot (eerste versie) en de twee layoutfixes; 0.4.0 is de definitieve release hiervan.

## 0.3.1 - 2026-10-03

- Nieuw: **update-melding bij het starten**. Eén verzoek aan GitHub Releases; bij een nieuwere
  versie een melding in de huisstijl met een regel over wat er nieuw is, Downloaden, Later en
  "10 starts niet tonen".

## 0.3.0 - 2026-10-03

- Nieuw: **ingebouwde DHCP-server** (tab DHCP). Zet de gekozen kaart op 192.168.8.8 en deelt
  uit vanaf 192.168.8.100 (50 adressen); uitgegeven adressen live met MAC en hostnaam; altijd
  een waarschuwing vooraf; luistert alleen op de gekozen kaart; bij uitschakelen krijgt de
  kaart zijn oude instellingen terug.
- Nieuw: **zes talen** (Nederlands, Engels, Duits, Frans, Italiaans, Spaans), standaard de
  taal van Windows; Instellingen-tab met tandwiel.
- Opgelost: T8-Lan startte niet meer mee na een herstart (eigen starttaak zonder de
  accu-beperkingen van Windows); meerdere exemplaren tegelijk (nu één instantie); online- en
  offline-status als stip op het tray-icoon, pas rood na twee gemiste pings.
- Venster vrij te vergroten en te verkleinen; opent compact.
- Netwerkscan: tweede ronde voor gemiste apparaten, ONVIF-herkenning van vrijwel elk
  cameramerk, Hikvision SADP v3.1 met firmwareversie en activeringsstatus.
- Programma heet `T8-Lan.exe`; bevestigingsvragen in de huisstijl; installer 1,3 MB.

## 0.2.0 - 2026-05-28

- Fieldtest. Soepeler slepen van het venster; LICENSE en README (directe downloadlink,
  bedrijfsnaam Turn8.io) bijgewerkt.

## 0.1.0 - 2026-05-22

- Eerste fieldtest: snel schakelen tussen statisch IP en DHCP per netwerkkaart (ook via het
  tray-menu), netwerkscan met apparaatherkenning (camera's en NVR's met merk en type,
  alarmpanelen), live ping-test met grafiek en statistieken. Geen account, geen telemetry,
  geen cloud.
