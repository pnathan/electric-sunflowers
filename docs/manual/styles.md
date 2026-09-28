---
title: Styles
description: The 22 styles, with their meters, tempo ranges, modes, bands, break instruments and forms.
---

[Manual](./) | Previous: [The command line](command-line.md) | Next: [Troubleshooting](troubleshooting.md)

# Styles

A style fixes what Claude is asked to write and how the band plays it. When a song is written, the seed picks one of the style's meters, then one of its forms and one of its modes; the tempo range is the one for that meter. When a song is rendered with a style, the style's guitar pattern, drums, band parts and break instrument replace the song's own, and the tempo is clamped to the style's range for the song's meter, widened by 10% each way (a cowboy song in 3/4 at 120 bpm plays at 114).

The tables are generated from `crates/songwriter/src/styles.rs`. The key is what `--style` takes; the name is what the New song form lists.

The lead guitar and the singer play in every style. "Other parts" are the parts added to them: the bass; a second (harmony) guitar; harp; violin; the choir (four parts of three singers each, on the vowel /aa/, over repeated choruses, bridges and outros); harmonies (a second voice a third to a sixth from the melody in the choruses); doubles (two more takes of the melody in repeated choruses, panned left and right). In a form without a chorus, the verses after the first count as choruses here. Drums are none, brushes, soft or full. The break lead takes the tune in instrumental breaks; "violin and guitar" trade it. Tempos are in felt beats per minute: in 6/8, dotted quarters.

| Key | Style | Meters, tempo (bpm) | Modes | Guitar | Drums | Other parts | Break lead |
|---|---|---|---|---|---|---|---|
| `appalachian` | Appalachian ballad | 3/4 72-96; 4/4 66-88 | mixolydian, dorian, minor, major | fingerpick | none | violin | violin |
| `oldtime` | Old-time string band | 4/4 100-128 | major, mixolydian | strum | none | bass, harmony guitar, violin, harmonies | violin |
| `bluegrass` | Bluegrass | 4/4 108-140; 3/4 100-132 | major | strum | none | bass, harmony guitar, violin, harmonies | violin and guitar |
| `cowboy` | Western and cowboy song | 3/4 80-104; 4/4 76-100 | major | travis | brushes | bass, harmony guitar, violin, harmonies | violin |
| `bakersfield` | Bakersfield country | 4/4 112-144 | major, mixolydian | strum | full | bass, harmony guitar, violin, harmonies, doubles | guitar |
| `texas` | Texas songwriter | 4/4 78-104; 3/4 84-108 | major, minor | travis | brushes | bass, harmony guitar | guitar |
| `cajun` | Cajun waltz | 3/4 104-138 | major | strum | soft | bass, violin, harmonies | violin |
| `zydeco` | Creole and zydeco two-step | 4/4 108-132 | major, mixolydian | strum | full | bass, harmony guitar, violin, choir, harmonies, doubles | violin and guitar |
| `acadian` | Acadian fiddle song | 6/8 62-80; 3/4 96-120 | major, mixolydian | strum | soft | bass, violin, harmonies | violin |
| `broadside` | English broadside ballad | 6/8 52-68; 3/4 80-104; 4/4 76-98 | major, dorian, mixolydian | fingerpick | none | violin | violin |
| `scottish` | Scottish ballad | 3/4 66-88; 4/4 60-80 | dorian, mixolydian, minor | arpeggio | none | bass, harp, violin | violin |
| `irishair` | Irish air | 3/4 60-80; 6/8 44-60 | major, dorian, mixolydian | arpeggio | none | bass, harp, violin | violin |
| `irishpub` | Irish drinking song | 6/8 68-84; 4/4 104-128 | major, mixolydian | strum | soft | bass, violin, choir, harmonies, doubles | violin |
| `welsh` | Welsh hymn tune | 4/4 60-78; 3/4 66-84 | major | arpeggio | none | bass, harp, choir, harmonies | guitar |
| `breton` | Breton dance song | 4/4 104-128; 6/8 68-84 | dorian, minor | strum | soft | bass, violin, harmonies | violin |
| `blues` | Delta and Piedmont blues | 4/4 70-100 | mixolydian, major | travis | brushes | bass, harmony guitar | guitar |
| `gospel` | Gospel | 4/4 72-104; 6/8 50-66 | major | arpeggio | soft | bass, choir, harmonies, doubles | guitar |
| `revival` | 1960s folk revival | 4/4 92-124; 3/4 92-116 | major, minor | fingerpick | none | harmonies | guitar |
| `laurel` | Laurel Canyon | 4/4 72-100; 3/4 80-104 | major, mixolydian | arpeggio | soft | bass, harmony guitar, harmonies, doubles | guitar |
| `nashville` | Nashville country waltz | 3/4 84-112 | major | strum | brushes | bass, harmony guitar, violin, harmonies | violin |
| `americana` | Present-day Americana | 4/4 72-112; 3/4 84-110 | major, minor, mixolydian | strum | soft | bass, harmony guitar, violin, harmonies, doubles | violin and guitar |
| `shanty` | Sea shanty | 4/4 96-124; 6/8 60-76 | major, dorian | strum | none | violin, choir, harmonies, doubles | violin |

## Forms

Each style draws its form from this list.

| Key | Forms |
|---|---|
| `appalachian` | strophic ballad; verses with a refrain line |
| `oldtime` | verse and chorus with instrumental breaks; verses with a refrain line |
| `bluegrass` | verse and chorus with instrumental breaks |
| `cowboy` | verse and chorus; verse and chorus with a break; strophic ballad |
| `bakersfield` | verse and chorus with instrumental breaks; verse and chorus; chorus first |
| `texas` | verses with a refrain line; strophic ballad; AABA (32-bar song form) |
| `cajun` | verse and chorus with a break |
| `zydeco` | chorus first; verse and chorus with instrumental breaks |
| `acadian` | verse and chorus with instrumental breaks; verses with a refrain line |
| `broadside` | strophic ballad; verses with a refrain line |
| `scottish` | strophic ballad; verses with a refrain line; AABA (32-bar song form) |
| `irishair` | strophic ballad; AABA (32-bar song form) |
| `irishpub` | verse and chorus; chorus first; verse and chorus with instrumental breaks |
| `welsh` | hymn stanzas |
| `breton` | verses with a refrain line; verse and chorus with instrumental breaks |
| `blues` | 12-bar blues |
| `gospel` | chorus first; verse and chorus; verse, pre-chorus, chorus |
| `revival` | verses with a refrain line; verse and chorus; strophic ballad |
| `laurel` | verse and chorus; verse, pre-chorus, chorus; AABA (32-bar song form) |
| `nashville` | verse and chorus with a break; verse and chorus |
| `americana` | verse and chorus; verse, pre-chorus, chorus; chorus first; AABA (32-bar song form) |
| `shanty` | verses with a refrain line; chorus first |

The forms, as numbered plans in the prompt:

| Form | Sections |
|---|---|
| verse and chorus | intro, verse, chorus, verse, chorus, bridge, chorus, outro |
| verse and chorus with instrumental breaks | intro break, verse, chorus, interlude break, verse, chorus, interlude break, chorus, tag |
| strophic ballad | intro, five verses with an interlude before the fourth, outro; no chorus |
| verses with a refrain line | intro, three verses, interlude, verse, outro; each verse ends on the same refrain line |
| AABA (32-bar song form) | intro, verse, verse, bridge, verse, interlude, bridge, verse, outro |
| chorus first | chorus, verse, chorus, verse, chorus, chorus, outro |
| verse, pre-chorus, chorus | intro, verse, pre-chorus, chorus, verse, pre-chorus, chorus, bridge, chorus, outro |
| verse and chorus with a break | intro break, verse, chorus, interlude break, verse, chorus, chorus, outro |
| 12-bar blues | intro, two three-line verses, interlude break, two verses, outro; each line spans four bars |
| hymn stanzas | intro, two stanzas, interlude, two stanzas, a one-line tag; hymn meter (8.6.8.6 or 8.7.8.7) |

## World flavour

About 12% of written songs are seasoned with one of: fado, Cape Verdean coladeira, Mexican son jarocho, Tex-Mex conjunto, French chanson, klezmer, Malian desert blues. The seed decides. The style stays the frame.
