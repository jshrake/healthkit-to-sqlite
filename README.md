# healthkit-to-sqlite

[![ci](https://github.com/jshrake/healthkit-to-sqlite/actions/workflows/ci.yml/badge.svg)](https://github.com/jshrake/healthkit-to-sqlite/actions)
[![crates.io](https://img.shields.io/crates/v/healthkit-to-sqlite-cli.svg)](https://crates.io/crates/healthkit-to-sqlite-cli)
[![Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Command-line tool to convert Apple HealthKit data to a SQLite database.

![workout-routes-examples](/workout-routes-example.webp)

## Getting Started

1. Open the [Health](https://www.apple.com/ios/health/) app on your iOS device.
2. Click your profile icon in the top-right corner.
3. Click the "Export All Health Data" button.
4. Share the resulting ZIP archive to your computer.
5. Run healthkit-to-sqlite on the exported ZIP archive.

```bash
cargo install healthkit-to-sqlite-cli
healthkit-to-sqlite export.zip sqlite://healthkit.db
```

Please [create an issue](https://github.com/jshrake/healthkit-to-sqlite/issues/new) for all bugs, feature requests, or feedback.

### Datasette

You can use <https://datasette.io/> to view and explore the resulting SQLite database file. Install the <https://datasette.io/plugins/datasette-geojson-map> plugin to visualize the workout routes data on a map.

```bash
datasette install datasette-geojson-map
datasette healthkit.db
```

## Example Queries

Here are a few example SQL queries to help you start exploring your HealthKit data:

* Total walking, running, and hiking workout duration in hours for the month of December 2022:

```sql
select
  sum(duration) / 60 as total_duration
from
  workout
where
  (
    creationDate between '2022-12-01' and '2022-12-31'
  )
  and (
    workoutActivityType = 'HKWorkoutActivityTypeWalking' or
    workoutActivityType = 'HKWorkoutActivityTypeRunning' or
    workoutActivityType = 'HKWorkoutActivityTypeHiking'
  );
```

* Total distance covered in miles across all workouts for the month of December 2022:

```sql
select
  sum(
    json_extract(
      workoutStatistics,
      "$.HKQuantityTypeIdentifierDistanceWalkingRunning.sum"
    )
  ) as total_distance_miles
from
  workout
where
  (
    creationDate between '2022-12-01'
    and '2022-12-31'
  );
```

* The JSON data in the `workoutStatistics` column looks like:

```json
{
    "HKQuantityTypeIdentifierActiveEnergyBurned": {
        "endDate": "2019-12-27 13:10:51 -0800",
        "startDate": "2019-12-27 12:30:15 -0800",
        "sum": 135.70199584960938,
        "type": "HKQuantityTypeIdentifierActiveEnergyBurned",
        "unit": "Cal"
    },
    "HKQuantityTypeIdentifierBasalEnergyBurned": {
        "endDate": "2019-12-27 13:10:51 -0800",
        "startDate": "2019-12-27 12:30:15 -0800",
        "sum": 67.24250030517578,
        "type": "HKQuantityTypeIdentifierBasalEnergyBurned",
        "unit": "Cal"
    },
    "HKQuantityTypeIdentifierDistanceWalkingRunning": {
        "endDate": "2019-12-27 13:10:51 -0800",
        "startDate": "2019-12-27 12:30:15 -0800",
        "sum": 1.4269200563430786,
        "type": "HKQuantityTypeIdentifierDistanceWalkingRunning",
        "unit": "mi"
    }
}
```

## Decisions

* Only the `Record`, `Workout`, and `ActivitySummary` elements are currently exported.
* `Record` elements are inserted to a table with a name matching the value of the element's `type` attribute.
* `Workout` elements are inserted to a table named "Workout".
  * The descendent `workoutEvent` and `workoutStatistics` elements are represented as JSON columns.
  * The descendent `workoutRoute` element is converted to a GeoJSON LineString and stored in a JSON column named "geometry" for easy integration with <https://datasette.io/plugins/datasette-geojson-map>.
* `ActivitySummary` elements are inserted as rows to a table named "ActivitySummary".
* In an attempt to future proof against Apple adding, removing, or changing element attributes, the code only assumes the existence of a limited number of attributes:
  * `Record` elements must have a `type` attribute.
  * `Workout` elements must have a `workoutActivity` attribute.
  * `MetadataEntry` elements must have `key` and `value` attributes.
  * `FileReference` elements must have a `path` attribute.

## License

This project is licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](/LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
* MIT license ([LICENSE-MIT](/LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.
