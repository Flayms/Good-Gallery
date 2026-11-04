# Face models

> [!NOTE]
> Licenses marked _open_ are not settled. This isn't legal advice.

Good Gallery finds faces in two steps, and you choose a model for each. One model **detects** where the faces are. The other **recognizes** them: it turns each face into a list of numbers, and similar numbers suggest the same person.

Models run on your computer. The app connects to the internet only to download a model you asked for. Photos, faces and those numbers are never uploaded, and the numbers are never written into your files.

A name reaches your file only when you confirm it. Suggestions, faces you ignore and faces nobody has named stay in the app's index.

## Which should I pick?

| You want | Detection | Recognition |
| --- | --- | --- |
| **Speed** | YuNet | EdgeFace S |
| **Accuracy** | RetinaFace MobileNetV2 | EdgeFace Base |
| A clear license for work | YuNet | SFace |

All of these have a download button in _Settings › Faces_.

### Detection

![Detection models: time per photo against large faces found. YuNet is fastest, RetinaFace MobileNetV2 finds more, RetinaFace ResNet34 is slowest.](face-models/detection.svg)

- **For speed: YuNet.** It's about 5 times as fast as RetinaFace MobileNetV2 and smaller than most photos.
- **For accuracy: RetinaFace MobileNetV2.** It finds more faces of every size. RetinaFace ResNet34 looks better here, but that 100% rests on only 25 faces. It takes 4 times as long and found fewer medium-sized faces, so it isn't worth the wait, and it has no button.

### Recognition

![Recognition models: time per face against known people named. EdgeFace XS, S and Base and buffalo_l form the best trade-offs. SFace and AuraFace are beaten by another model.](face-models/recognition.svg)

- **For speed: EdgeFace S.** EdgeFace XS is 9 ms per face faster, but names 1.9% fewer people. Those 9 ms hardly matter next to the 0.7 s it takes to read a large photo.
- **For accuracy: `buffalo_l` measures best, but EdgeFace Base is the one to use.** `buffalo_l` is for research only (see Licenses). EdgeFace Base is 0.2 points behind and 5 times as fast.
- **For work: SFace.** It's the only one with a clear commercial license, though EdgeFace beats it on both speed and accuracy.

AuraFace did worst, and is the largest and slowest, so it has no button.

Changing the recognition model means reading every face again, which can take many hours on a large library. Names you confirmed are kept.

## Licenses

The license of a model's weights can differ from the license of its code. Only YuNet and SFace have a clear statement from their publisher.

- **YuNet:** MIT, from OpenCV.
- **SFace:** Apache-2.0, from OpenCV.
- **RetinaFace:** the repository's MIT license mentions software, not weights. Its weights were trained on WIDER FACE, whose terms appear to be non-commercial. YuNet was trained on the same data.
- **AuraFace:** the publisher labels the repository Apache-2.0 and says it was trained on a commercial dataset, which it doesn't name. Use only `glintr100.onnx`. The same repository holds InsightFace's detector and other files, which are non-commercial.
- **EdgeFace:** Idiap's model pages say CC BY-NC-SA 4.0 and its code license says BSD-3-Clause. Treat the weights as non-commercial. The download comes from a third party's conversion, as Idiap publishes no ONNX file.
- **buffalo_l:** InsightFace says its pretrained models are "for non-commercial research purposes only". Organizing your own photos may not count as research, so read [InsightFace's license terms](https://github.com/deepinsight/insightface#license) or ask them before using it. Good Gallery doesn't offer this model. Using it is your decision and your responsibility.

If you use Good Gallery for work, such as for a photography business, non-commercial models are not for you. Privacy law may also treat faces of other people differently than it does for a private collection.

## Using a model that isn't listed

In _Settings › Faces › Add another model_, paste an `https` link or choose an ONNX file you downloaded yourself. The app works out by itself whether it finds or recognizes faces. Detection models need code for their output, so only YuNet's and RetinaFace's formats work.

The one thing a file doesn't say is what pixel values a recognition model expects:

- **From -1 to 1:** most models, including `buffalo_l` and EdgeFace.
- **From 0 to 255:** SFace and models like it.

A wrong choice doesn't cause an error, it just gives poor results. If you have named faces, the app checks whether they still match and warns you if they don't.

Only add files you trust.

## Measurements

All of this ran through Good Gallery's own steps on one small machine: 2 shared CPU cores and 4 GB of memory, one model at a time, with no graphics card. A modern desktop will be several times faster, but the models should rank about the same. To repeat it, run `cargo xtask face-bench`.

The photos are public test sets, not family albums, so expect lower results on your own photos, especially for children and for faces far apart in age. Numbers measured the same way can be compared with each other, unlike the figures models publish about themselves.

### Detection models

Faces found are from every 10th photo of WIDER FACE's validation set (323 photos), every 30th (108) for ResNet34. These photos are often crowded and hard. Faces under 24 pixels don't count, because the app drops them on purpose. A false box is one on no labeled face, though some of those may be real faces nobody labeled.

| Model | Download | One look | Average per photo | Memory | Faces found: 24–63 px | 64–159 px | 160+ px | False boxes per 100 photos | License |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| YuNet | 233 KB | 90 ms | 0.5 s | 37 MB | 69% | 84% | 91% | 2.8 | MIT |
| RetinaFace MobileNetV2 | 12.5 MB | 0.7 s | 2.7 s | 118 MB | 76% | 93% | 98% | 5.6 | MIT (_open_) |
| RetinaFace ResNet34 | 88.5 MB | 2.8 s | 10.4 s | 234 MB | 79% | 89% | 100% | 1.9 | MIT (_open_) |

A photo gets one look, or more if it seems to hold faces too small or too large for the first. Better landmarks help recognition only a little: LFW scores rose from 99.30% to 99.52% for SFace and from 99.65% to 99.73% for EdgeFace S with RetinaFace MobileNetV2 instead of YuNet.

### Recognition models

Not every model here has a download button: see _Which should I pick?_. The columns from _Known person_ on come from a simulation with the app's own suggestions and groups, on LFW's photos: 1,739 known faces of 780 people, and 5,182 faces of strangers.

- **Known person named:** a new photo of someone known gets their name. At these thresholds, none got a wrong name in this test.
- **Stranger named:** someone unknown gets a suggestion anyway.
- **Clean groups:** faces in a group that belong to its main person.

| Model | Download | Per face | Memory | LFW | Known person named | Stranger named | Clean groups | Threshold | License |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| SFace | 38.7 MB | 112 ms | 110 MB | 99.30% | 95.9% | 0.4% | 98.8% | 0.50 | Apache-2.0 |
| EdgeFace XS | 7.3 MB | 30 ms | 41 MB | 99.55% | 96.4% | 0.2% | 99.3% | 0.50 | Non-commercial (_open_) |
| EdgeFace S | 14.8 MB | 39 ms | 53 MB | 99.65% | 98.3% | 0.2% | 98.8% | 0.50 | Non-commercial (_open_) |
| EdgeFace Base | 73.1 MB | 82 ms | 168 MB | 99.80% | 98.8% | 0.2% | 99.6% | 0.40 | Non-commercial (_open_) |
| `buffalo_l` (ResNet50) | 174.4 MB | 421 ms | 353 MB | 99.85% | 99.0% | 0.4% | 99.1% | 0.35 | Research only |
| AuraFace | 260.7 MB | 715 ms | 519 MB | 99.20% | 89.4% | 0.2% | 97.9% | 0.45 | Apache-2.0 (_open_) |

A model you add yourself uses a threshold of 0.50, which held for every model here.

### How long a library takes

Roughly, for 100,000 photos of 24 megapixels on the machine above, with 1.5 faces per photo:

| Detection + recognition | Hours |
| --- | --- |
| YuNet + EdgeFace S | 33 |
| YuNet + SFace | 36 |
| YuNet + `buffalo_l` | 49 |
| YuNet + AuraFace | 62 |
| RetinaFace MobileNetV2 + SFace | 97 |
| RetinaFace ResNet34 + AuraFace | 338 |

Reading the photos takes about 18 of those hours (0.66 s each), and more over a slow network. That figure comes from one test JPEG that compresses well, so real photos may take longer. The search also holds a decoded photo in memory, about 190 MB at this size.

## Sources

[YuNet](https://huggingface.co/opencv/face_detection_yunet) · [SFace](https://huggingface.co/opencv/face_recognition_sface) · [RetinaFace](https://github.com/yakhyo/retinaface-pytorch) · [AuraFace](https://huggingface.co/fal/AuraFace-v1) · [EdgeFace](https://github.com/otroshi/edgeface) · [EdgeFace model page](https://huggingface.co/Idiap/EdgeFace-Base) · [LFW](https://vis-www.cs.umass.edu/lfw/) · [InsightFace model zoo](https://github.com/deepinsight/insightface/blob/master/model_zoo/README.md) · [WIDER FACE](https://huggingface.co/datasets/CUHK-CSE/wider_face)
