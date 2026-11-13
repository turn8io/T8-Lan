# Wijzigingen per versie

Nieuwste bovenaan. De datum is de publicatiedatum van de release op GitHub; de installer
heet steeds `T8-Lan-v<major.minor>-setup.exe`. Bijwerken hoort bij elke release, in dezelfde
commit (zie `CLAUDE.md`).

## 0.6.0 - 2026-11-13

### Nieuw
- Netwerkscan in een **ander bereik**: het driehoekje in de scanknop opent een keuzemenu
  met "Eigen subnet" en je eigen bereiken ("192.168.101.1-254" of "192.168.101.0/24").
  Bereiken voeg je daar toe of verwijder je; ze blijven bewaard. Zo'n scan loopt via de
  router, met ping, poortchecks en ONVIF per adres; ARP en SADP werken daar niet en dat
  staat erbij.
- **ONVIF-probe rechtstreeks per adres**, naast de multicast. Camera's geven zo hun type
  terug, ook als multicast niet aankomt en ook vanuit een ander VLAN.
- Waarschuwing in de scan als SADP niet kon luisteren (poort 37020 bezet) of als de
  firewallregel voor de draaiende exe ontbreekt.

### Gewijzigd
- Het scanbereik volgt het **subnetmasker** van de adapter (tot /22, 1022 adressen) in
  plaats van altijd het /24 rond het adapter-IP.
- ARP-sweep en TCP-checks lopen via de **gekozen netwerkkaart**. Op een pc met meerdere
  kaarten koos Windows er eerder zelf een, waardoor een scan vrijwel leeg kon blijven.
- Het cameratype komt uit de ONVIF-scope `hardware` (altijd gevuld) in plaats van `name`
  (aanpasbaar, soms leeg), met het merk ervoor: "Hikvision DS-2CD2387G3-LIS2UY/SL" waar
  eerder alleen het kale typenummer stond.
- Merk en type zijn gescheiden velden. Bij meerdere bronnen voor hetzelfde IP wint de
  betrouwbaarste (apparaat zelf, SADP, ONVIF-hardware, ONVIF-naam, MAC-adres) in plaats van
  het label met een spatie erin.
- Vijf Hikvision-MAC-reeksen toegevoegd: 08:CC:81, 0C:75:D2, 04:EE:CD, 88:DE:39, CC:13:F3.
- Een pc of printer die WS-Discovery spreekt zonder ONVIF telt niet meer als camera.
- **Ping** begint met het subnet van de netwerkkaart ("192.168.62.") in plaats van 1.1.1.1:
  alleen het laatste getal intikken om te zien of een adres vrij is.
- Ping toont vóór de start "Stel een IP-adres in en druk op Start" in plaats van
  "Wachten op data...".
- De statuskaart toont een probleem in plaats van waarden als er niets te tonen valt:
  **"Geen IP ontvangen"** (DHCP zonder server, met het noodadres klein eronder en een
  knop **"Opnieuw aanvragen"** die `ipconfig /renew` doet, met een limiet van 12 seconden)
  en **"Geen verbinding"** (geen link op de kaart). Een monteur ziet zo meteen dat 169.254.x.x geen echt IP is.

### Opgelost
- Een adapter met DHCP aan maar zonder DHCP-server (Windows-noodadres 169.254.x.x, APIPA)
  werd als "statisch" getoond, en "Schakel naar DHCP" faalde met "DHCP is already enabled".
  De app leest nu de DHCP-vlag van de adapter in plaats van het antwoord van de server.

### Beveiliging
- Geen nieuwe meldingen met een oplossing sinds 0.5.1 (updatecheck van 9-11-2026); de regels
  zonder bekende oplossing staan nog open.

## 0.5.1 - 2026-11-13

### Nieuw
- Netwerkscan herkent **intercoms**, zonder inloggen: **2N** (type, firmware en serienummer
  via het open `/api/system/info`) en **Zenitel/Stentofon** (merk en firmware uit de
  StationWeb-loginpagina), met het MAC-adres als vangnet voor beide merken. Een 2N werd
  eerder als "camera, merk onbekend" getoond.

### Gewijzigd
- De http(s)-probes van de scan lopen via WinHTTP (de HTTP-stack van Windows zelf), dus
  https naar apparaten met een self-signed certificaat werkt zonder extra crates.
- Repo: geen GitHub Actions-build meer, releases worden lokaal gebouwd en ondertekend; dode
  scripts (`rename-installer.ps1`, `scripts\sign.ps1`, npm-script `release`) en de
  ongebruikte vertaalsleutel `dns.custom` verwijderd; `CLAUDE.md` en dit bestand toegevoegd.

### Beveiliging
- vite 6.4.5 en quick-xml 0.41.0 (via wayland-scanner 0.31.11), alleen versies van minstens
  7 dagen oud. glib en event-listener uit de lijst zijn Linux-only en zitten niet in de
  Windows-installer.

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
