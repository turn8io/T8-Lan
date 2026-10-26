## Beveiligingsupdates

De openstaande bekende lekken in dit project staan in
`P:\projects\T8-Updatecheck\projecten\T8-LAN.md` (elke maandag ververst). Lees die bij de
start en werk vóór elke release elke regel met een oplossing weg; "later" bestaat niet,
want elke extra release kost iedere gebruiker tijd. Alleen versies die minstens 7 dagen
gepubliceerd zijn: npm dwingt dat af via `.npmrc` (`min-release-age=7`), voor cargo
`cargo update -p <crate> --precise <versie>`. Regels zonder oplossing blijven staan.

## Releases

Lokaal bouwen en ondertekenen, nooit met GitHub Actions. Volgorde, inrichting en valkuilen
staan in de skills `turn8-github-release` en `turn8-signing`. Houd `CHANGELOG.md` bij per
versie, in dezelfde release.
