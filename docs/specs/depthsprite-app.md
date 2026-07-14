# DepthSprite application specification

## Purpose

DepthSprite is a model-authoring application for pixel artists. It turns a small
set of orthographic sprites into a rotatable pseudo-3D model while keeping those
sprites as the authored source.

PNG color defines the visible art. Inverted PNG alpha defines per-pixel relief, so
the same chart can describe rounded profiles, protrusions, divots, and recessed
surfaces.

## Model artifact

Each model is one `.depthsprite` file. The file contains the model bounds and one
or more canonical PNG charts. Its internal image names and organization belong to
the format, so opening or moving a model is a one-file operation.

All models share the same program-wide relief scale. A file carries authored image
data and model dimensions, with no model-specific interpretation controls.

## Authoring workflow

1. Create a model by assigning PNG charts to canonical sides, or open an existing
   `.depthsprite` model.
2. Add, replace, remove, and edit the model's charts.
3. See each change immediately in the transformed model view.
4. Orbit the result to inspect silhouette, color, protrusions, recesses, and chart
   transitions.
5. Save the working model as one `.depthsprite` file.
6. Reopen that file with the same authored charts and model behavior.

The first interaction-design study will determine the most direct division between
in-app pixel editing and round trips through a dedicated pixel editor. It will use
real model-authoring tasks and preserve the workflow above.

## Interface

The working model is the primary visual surface. Orbit and zoom operate directly
on that surface. Chart selection and editing stay visually connected to their
effect on the model. File actions use standard, recognizable placement and names.

The interface presents information when it supports the current authoring action.
Model dimensions appear where they are created or changed. Chart identity appears
where charts are selected or edited. The canvas retains the rest of the available
space.

## Required behavior

- A model contains any nonempty subset of front, right, back, left, top, and bottom
  charts.
- Chart image dimensions agree with the model bounds and canonical orientation.
- Alpha zero is background. Each nonzero-alpha pixel supplies its RGB color and an
  inverted-alpha relief sample.
- Orbiting computes a new image from the current charts without changing them.
- The same model and camera state always produce the same pixels and overlap owners.
- Chart storage order cannot change the rendered result.
- A front-and-top bowl displays a rounded exterior and a visibly recessed basin.
- Saving and reopening preserve the model's bounds, views, colors, transparency,
  and relief samples.

## Acceptance

A user can create or open a model, work on its depth-bearing charts while seeing
the transformed result, orbit the result for inspection, save one model file, and
reopen that file with the same authored model.

The reference bowl is the decisive relief example. Its front chart supplies the
rounded wall and rim; its top chart supplies the recessed basin. Both remain
visibly responsible for the combined image at an oblique view.

The repository currently implements the model representation, package persistence,
transformation renderer, deterministic compositor, and reference examples. The
authoring surface is the next implementation phase.
