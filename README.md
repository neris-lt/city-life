# Neon Vilnius

A top-down open-world crime game written in Rust with [Bevy](https://bevyengine.org) 0.20,
running in the browser via WebAssembly/WebGL2. The map is a 1:1 scale reconstruction of the
whole of Vilnius (about 147,000 buildings, 19,000 road segments, real street names) built
from OpenStreetMap data and restyled as a rainy neon pixel-art night. You start in Šeškinė.

## Run

    ./build_web.sh                       # builds web/neon_district.js + .wasm (slow, size-optimised)
    PROFILE=web-dev ./build_web.sh       # quicker build for iterating
    python3 -m http.server 8765 --directory web

Then open http://localhost:8765. Needs `rustup target add wasm32-unknown-unknown` and
`cargo install wasm-bindgen-cli --version 0.2.130`.

## Controls

WASD move / drive, mouse aim + shoot, F enter/exit car, Space handbrake, Shift sprint,
Q switch weapon, T skip an hour, mouse wheel or + / - (or the on-screen buttons) zoom, 0-9 fast travel (on foot).

**M** opens the city map: scroll to zoom, drag to pan, click to mark a destination (right-click
clears it). A GPS route along real streets then shows on the map, the minimap and the road.

## Weather

The weather drifts between drizzle, rain, downpour, thunderstorm and fog. Wind slants the rain,
lightning lights the city and throws hard shadows, streets get glossier the harder it rains,
and tyres throw spray. The current front is shown in the bottom-left HUD line.

## Speed cameras

The 51 speed cameras mapped in Vilnius stand at their real locations and enforce the posted limit
of their road (limits come from the map data, defaulting to 50 km/h). Pass one more than 9 km/h
over the limit and it flashes and fines you. A speedometer with the current limit shows while you
drive, and cameras appear as orange markers on the minimap.

## Graphics settings

The game renders at the display's native resolution (up to 1440 lines) with anti-aliasing.
**G** cycles the render scale (100% / 75% / 50%) if you need more speed.

## Day and night

One in-game hour passes per real minute (the clock is in the bottom-left HUD line; **T** skips an
hour). The sun crosses east to west with warm light at dawn and dusk; street lamps, neon, windows
and headlights fade out by day and come back at night.

## Public transport

Buses, trolleybuses and night buses follow their real Vilnius routes and stops, taken from the
city's GTFS feed (`tools/build_transit.py` -> `data/transit.bin`). How many run on each route
follows the real weekday timetable for the current in-game hour (night buses only in the small
hours). Because the in-game day is compressed, services run four times closer together than in
reality. Each vehicle shows its route number in lights on the roof and as a green dot on the
minimap.

## Terrain relief

Real elevation data (SRTM, `tools/build_relief.py` -> `data/relief.bin`) shades the ground and the
city map so hills and valleys read at a glance, and the HUD shows your altitude. The world is
built on that terrain at true scale: ground, roads, buildings, bridges, vehicles, people and the
camera all follow the real hills and valleys (driving physics itself ignores slope).

## Airport and landmarks

`tools/build_extras.py` turns `data/landmarks_osm.json` (OpenStreetMap `aeroway` features and a few
building footprints) into `data/extras.bin`, which is embedded in the game: Vilnius airport's runway,
taxiways, apron and stands with parked airliners, and hand-modelled landmarks on their real
footprints (Cathedral and bell tower, Gediminas' Tower, Three Crosses, TV Tower, Europa Tower,
St. Anne's, St. Peter and St. Paul's, Town Hall, Gate of Dawn, Opera, Palace of the Grand Dukes).
Tall ones are scaled down to stay below the overhead camera.

## How the map works

- `data/core.bin` (embedded in the binary): the drivable road graph, street names, rivers, tile index.
- `web/assets/map/<x>_<z>.bin`: 512 m vector tiles (roads, footpaths, building footprints with
  floor counts, parking, water, forests, railways, mapped trees, traffic signals, bus stops,
  roundabout islands). Bridges and tunnels carry per-point heights: roads ramp up onto decks or
  down into cuttings, and vehicles on different levels pass over each other. The game fetches the tiles around the camera and
  meshes them in 64 m cells, so only the neighbourhood you are in is ever in memory.

To rebuild the map from fresh OpenStreetMap data:

    python3 -m venv .venv && .venv/bin/pip install osmium
    curl -L -o data/osm/lithuania.osm.pbf https://download.geofabrik.de/europe/lithuania-latest.osm.pbf
    .venv/bin/python tools/extract_pbf.py      # cuts Vilnius out of the country extract
    python3 tools/build_map.py                 # writes data/core.bin and web/assets/map/

(`tools/fetch_osm.sh` is an alternative that pulls the same data from the Overpass API.)
To use another city, change the bounding box in `tools/extract_pbf.py` and the origin
(`LAT0`/`LON0`) in `tools/build_map.py`.

Map data (c) OpenStreetMap contributors, available under the Open Database License (ODbL).
