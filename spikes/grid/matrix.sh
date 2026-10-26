#!/usr/bin/env bash
# Full M0 grid matrix. Results: results/*.json, summary lines on stdout.
export SECONDS_=20
T="--threads 4"
for view in justified masonry; do
  for cols in 5 12; do
    for speed in 1440 6000; do
      ./bench.sh "$view-c$cols-s$speed" --view $view --cols $cols --speed $speed --vsync off $T
    done
  done
done
./bench.sh "justified-c12-s1440-250k" --items 250000 --cols 12 --speed 1440 --vsync off $T
./bench.sh "justified-c12-s1440-vsync" --cols 12 --speed 1440 --vsync on $T
./bench.sh "justified-c12-s6000-vsync" --cols 12 --speed 6000 --vsync on $T
./bench.sh "justified-c12-s1440-egui" --cols 12 --speed 1440 --vsync off --upload egui $T
./bench.sh "justified-c12-s1440-imagers" --cols 12 --speed 1440 --vsync off --decoder image $T
./bench.sh "justified-c12-s1440-t8" --cols 12 --speed 1440 --vsync off --threads 8
./bench.sh "justified-c12-s1440-cold" --cols 12 --speed 1440 --vsync off --warm off $T
./bench.sh "justified-c5-s1440-cold" --cols 5 --speed 1440 --vsync off --warm off $T
