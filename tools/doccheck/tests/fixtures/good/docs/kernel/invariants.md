# Invariants

## Purpose

What always holds.

## The invariants

### I1 (handles name live objects)

Status: built · partly tested: races between harts are not attacked · tested: mutation:SkipCheck, fuzz:demo/parse

Every handle names a live object.

### I2 (every flow obeys R1)

Status: built · tested: host:demo::flows

The model checks R1 (flow) after every step.

## Why

Because.
