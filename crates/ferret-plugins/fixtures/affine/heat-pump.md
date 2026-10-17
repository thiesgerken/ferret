# Heat pump service

The heat pump in the basement gets a **yearly service** in _October_. The installer's manual is at [heatpump.example](https://heatpump.example/manual), and the error code `E42` means low brine pressure. See [ferret-linked](https://affine.example/workspace/w1/hGhAvlHvXN) for readings.

## Checklist

- Read the brine pressure
    - It should sit between 1.5 and 2.5 bar
    - Below 1 bar the pump stops
- Clean the filter
- Note the meter reading
1. Switch off the pump
2. Open the valve
3. Wait ten minutes
- [x] Book the technician
- [ ] Pay the invoice

**Service log**

| Title |
|---|
| 2025 service: filter replaced |
| 2026 service: brine topped up |

> Never refill the brine with tap water.

```shellscript
curl -s http://heatpump.local/status
```

---

| Part | Interval | Cost |
|---|---|---|
| Filter | yearly | 12 EUR |
| Brine | every five years | 80 EUR |
