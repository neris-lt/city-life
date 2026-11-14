#!/bin/sh
# Downloads OpenStreetMap data for the whole of Vilnius as a 8x8 grid of Overpass extracts
# into data/osm/. Re-run safe: finished chunks are skipped.
cd "$(dirname "$0")/.."
LAT0=54.58; LAT1=54.82; LON0=25.05; LON1=25.44; N=8
for i in 0 1 2 3 4 5 6 7; do for j in 0 1 2 3 4 5 6 7; do
  out="data/osm/chunk_${i}_${j}.json"
  [ -s "$out" ] && tail -c 20 "$out" | grep -q '}' && head -c 200 "$out" | grep -q '"version"' && continue
  s=$(echo "$LAT0 + ($LAT1 - $LAT0) * $i / $N" | bc -l); n=$(echo "$LAT0 + ($LAT1 - $LAT0) * ($i + 1) / $N" | bc -l)
  w=$(echo "$LON0 + ($LON1 - $LON0) * $j / $N" | bc -l); e=$(echo "$LON0 + ($LON1 - $LON0) * ($j + 1) / $N" | bc -l)
  for try in 1 2 3 4 5 6 7 8; do
    case $((try % 3)) in 1) EP=https://overpass.private.coffee/api/interpreter;; 2) EP=https://overpass-api.de/api/interpreter;; *) EP=https://maps.mail.ru/osm/tools/overpass/api/interpreter;; esac
    curl -sS -m 240 -A "osm-map-fetch/1.0" --data-urlencode "data=[out:json][timeout:180][maxsize:1500000000][bbox:$s,$w,$n,$e];
(
  way[\"highway\"];
  way[\"building\"];
  relation[\"building\"][\"type\"=\"multipolygon\"];
  way[\"landuse\"~\"grass|forest|meadow|cemetery|recreation_ground|village_green\"];
  way[\"leisure\"~\"park|pitch|playground|garden\"];
  way[\"natural\"~\"wood|water|scrub|grassland\"];
  relation[\"natural\"=\"water\"];
  way[\"amenity\"=\"parking\"];
  way[\"waterway\"];
);
out geom;" "$EP" -o "$out"
    if head -c 200 "$out" | grep -q '"version"'; then echo "ok $out $(ls -lh "$out" | awk '{print $5}')"; break; fi
    echo "retry $out ($try): $(head -c 120 "$out" | tr '\n' ' ')"; sleep $((try * 8))
  done
  sleep 3
done; done
echo DONE
