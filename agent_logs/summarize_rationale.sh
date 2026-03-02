rg '"rationale"' tick_*/response.json \
| sort -V \
| sed 's/.*"rationale": "//; s/",$//'
