# Face models

> [!WARNING]
> Draft for #36. Speed and memory are not measured yet, and the licenses marked _open_ are not settled. Not legal advice.

Good Gallery finds faces in two steps, and you choose a model for each. One model **detects** where the faces are. The other **recognizes** them: it turns each face into a list of numbers, and similar numbers suggest the same person.

Models run on your computer. Photos, faces and those numbers are never uploaded, and the numbers are never written into your files.

## Which should I pick?

| You want | Detection | Recognition |
| --- | --- | --- |
| A small download and a clear license | YuNet | SFace |
| To find small faces in group photos | RetinaFace | SFace |
| Possibly better recognition, if you have time and disk space (see Licenses for AuraFace's open question) | RetinaFace | AuraFace |

Changing the recognition model means reading every face again, which can take many hours on a large library. Names you confirmed are kept.

## Detection models

| Model | Download | Finds faces in WIDER FACE (easy / medium / hard) | Speed | Memory | License |
| --- | --- | --- | --- | --- | --- |
| YuNet (2023) | 232 KB | 88.4% / 86.6% / 75.0% | not measured | not measured | MIT |
| RetinaFace, MobileNetV2 | not measured | 94.0% / 92.3% / 83.6% | not measured | not measured | MIT (_open_) |
| RetinaFace, ResNet34 | not measured | 95.1% / 93.5% / 84.4% | not measured | not measured | MIT (_open_) |

RetinaFace's figures are at the photo's original size. Scaling images for the test raises its _hard_ score (88.9% for ResNet34) and lowers _easy_ and _medium_. YuNet's model page also lists lower figures (83.4% / 82.4% / 70.8%) without explaining the difference.

## Recognition models

Higher is better. Results come from each publisher, with different detectors and test setups, so small differences mean little. CPLFW tests faces at different angles, and AgeDB tests faces at different ages.

| Model | Download | LFW | CPLFW | AgeDB-30 | Speed | Memory | License |
| --- | --- | --- | --- | --- | --- | --- | --- |
| SFace | 38.7 MB | 99.40%¹ | not published | not published | not measured | not measured | Apache-2.0 |
| AuraFace v1 | 260.7 MB | 99.65% | 90.93% | 96.10% | not measured | not measured | Apache-2.0 (_open_) |
| EdgeFace S (γ=0.5) | 14.7 MB² | 99.78% | 92.48% | 97.03% | not measured | not measured | Non-commercial (_open_) |
| EdgeFace Base | 73.0 MB² | 99.83% | 93.75% | 97.60% | not measured | not measured | Non-commercial (_open_) |
| buffalo_l (ResNet50) | 174.4 MB | 99.83% | not published | 98.23% | not measured | not measured | Research only |

¹ Result of OpenCV's evaluation tool. Its page doesn't name the dataset.
² PyTorch weights. The publisher offers no ONNX file, so the size may change when converted.

## Licenses

The license of a model's weights can differ from the license of its code. Only YuNet and SFace have a clear statement from their publisher.

- **YuNet:** MIT, from OpenCV.
- **SFace:** Apache-2.0, from OpenCV.
- **RetinaFace:** the repository's MIT license mentions software, not weights. Its weights were trained on WIDER FACE, whose terms appear to be non-commercial. YuNet was trained on the same data.
- **AuraFace:** the publisher labels the repository Apache-2.0 and says it was trained on a commercial dataset, which it doesn't name. Use only `glintr100.onnx`. The same repository holds InsightFace's detector and other files, which are non-commercial.
- **EdgeFace:** Idiap's model pages say CC BY-NC-SA 4.0 and its code license says BSD-3-Clause. Until Idiap clarifies this, treat the weights as non-commercial.
- **buffalo_l:** InsightFace says its pretrained models are "for non-commercial research purposes only". Organizing your own photos may not count as research, so read [InsightFace's license terms](https://github.com/deepinsight/insightface#license) or ask them before using it. Good Gallery doesn't offer this model. Using it is your decision and your responsibility.

If you use Good Gallery for work, such as for a photography business, non-commercial models are not for you. Privacy law may also treat faces of other people differently than it does for a private collection.

## Using a model that isn't listed

Choose **Add model file** and pick an ONNX file you downloaded yourself. You then choose how the model expects its input:

- **ArcFace style:** most recognition models. RGB values scaled to −1…1.
- **SFace style:** RGB values from 0 to 255.

A wrong choice doesn't cause an error, it just gives poor results. If you have named faces, the app checks whether they still match and warns you if they don't.

## Sources

[YuNet](https://huggingface.co/opencv/face_detection_yunet) · [SFace](https://huggingface.co/opencv/face_recognition_sface) · [RetinaFace](https://github.com/yakhyo/retinaface-pytorch) · [AuraFace](https://huggingface.co/fal/AuraFace-v1) · [EdgeFace](https://github.com/otroshi/edgeface) · [EdgeFace model page](https://huggingface.co/Idiap/EdgeFace-Base) · [InsightFace model zoo](https://github.com/deepinsight/insightface/blob/master/model_zoo/README.md) · [WIDER FACE](https://huggingface.co/datasets/CUHK-CSE/wider_face)
