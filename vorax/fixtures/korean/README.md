# Korean window regression fixtures

Screenshots supplied by the repository owner in the OCR bug report (2026-10-31).
Game artwork remains the property of its rights holders. These files contain the game window only.

| File | Round | Expected offers | Tank activity × quantity | Rerolls |
|---|---:|---|---|---|
| instrument-no-reroll.png | 1 | Incubation Sac; Hypertrophic Pituitary; Contracted Claws | 1×36; 5×24; 1×36; 1×36 | Control absent, pet level 0 |
| potion-round1.png | 1 | Spinal Solution - Aemberon; Slender-Limb Powder - Pest; Anesthetic Tincture - Aemberon | 1×36; 5×24; 1×36; 1×36 | 3 |
| potion-round4-ultrawide.png | 4 | Osteolytic Ointment; Twin Hormone - Pest; Potion Kit (Small) | 1×186; 5×328; 1×109 | 1 |

`data/scan/korean-round.json` adds one labelled round-4 glyph from the third frame to the adaptive font fallback. It does not contain a screenshot fingerprint or an assumed board state. The legacy reference reader continues to use its original samples unchanged.

`korean-titles.json` contains normalized, small title-shape vectors for the nine labelled offers. They can only resolve names already supported by the art candidates, with a strict similarity and separation threshold. Unseen titles remain the job of Windows Korean OCR. `korean-panel.json` supplements the known starting map-item icon and Contracted Claws. Starting items are excluded from the equipped instruments, as in the original reader.
