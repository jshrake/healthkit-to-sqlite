# healthkit-to-sqlite

[![ci](https://github.com/jshrake/healthkit-to-sqlite/actions/workflows/ci.yml/badge.svg)](https://github.com/jshrake/healthkit-to-sqlite/actions)
[![crates.io](https://img.shields.io/crates/v/healthkit-to-sqlite-cli.svg)](https://crates.io/crates/healthkit-to-sqlite-cli)
[![Rust](https://img.shields.io/badge/rust-1.66%2B-blue.svg)](https://github.com/jshrake/healthkit-to-sqlite)
[![Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Command-line tool to convert Apple HealthKit data to a SQLite database.

![workout-routes-examples](/workout-routes-example.webp)

## Getting Started

1. Open the [Health](https://www.apple.com/ios/health/) app on your iOS device.
2. Click your profile icon in the top-right corner.
3. Click the "Export All Health Data" button.
4. Share the resulting ZIP archive to your computer.
5. Run the healthkit-to-sqlite tool on the exported ZIP archive.

```bash
cargo install healthkit-to-sqlite-cli
healthkit-to-sqlite export.zip sqlite://healthkit.db
```

### Datasette

You can use <https://datasette.io/> to view and explore the resulting SQLite database file. Install the <https://datasette.io/plugins/datasette-geojson-map> plugin to visualize the workout routes data on a map.

```bash
datasette install datasette-geojson-map
datasette healthkit.db
```

## Decisions

* Only the `Record`, `Workout`, and `ActivitySummary` elements are currently exported.
* `Record` elements are inserted to a table with a name matching the value of the element's `type` attribute.
* `Workout` elements are inserted to a table named "Workout".
  * The descendent `workoutEvent` and `workoutStatistics` elements are represented as JSON columns.
  * The descendent `workoutRoute` element is converted to a GeoJSON LineString and stored in a JSON column named "geometry" for easy integration with <https://datasette.io/plugins/datasette-geojson-map>.
  * The JSON columns can be verbose and result in large HTML table cell sizes, making it difficult to browse the results. In this case, it may be useful to specify the [truncate_cells_html setting](https://docs.datasette.io/en/stable/settings.html#truncate-cells-html) `datasette healthkit.db --setting truncate_cells_html 40`. Note that this setting does not appear to be compatible with the [datasette-pretty-json plugin](https://datasette.io/plugins/datasette-pretty-json).
* `ActivitySummary` elements are inserted as rows to a table named "ActivitySummary".
* In an attempt to future proof against Apple adding, removing, or changing element attributes, the code only assumes the existence of a limited number of attributes:
  * `Record` elements must have a `type` attribute.
  * `Workout` elements must have a `workoutActivity` attribute.
  * `MetadataEntry` elements must have `key` and `value` attributes.
  * `FileReference` elements must have a `path` attribute.
