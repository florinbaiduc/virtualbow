#include "SettingsView.hpp"
#include "primitive/IntegerView.hpp"
#include "primitive/DoubleView.hpp"
#include "pre/utils/IntegerRange.hpp"
#include "pre/utils/DoubleRange.hpp"
#include "pre/models/units/UnitSystem.hpp"
#include "pre/models/SettingsModel.hpp"
#include "pre/models/MainTreeModel.hpp"
#include "pre/Language.hpp"
#include <QCheckBox>

namespace {
    // Build a checkbox whose state is bound to the given lower-limb top-level
    // row's symmetry flag. Toggling the box flips the flag via MainTreeModel,
    // which in turn hides / unhides the row, syncs upper → lower data, and
    // emits the necessary change signals.
    QCheckBox* makeSymmetryCheckBox(MainTreeModel* tree, int row, const QString& tooltip) {
        auto box = new QCheckBox();
        box->setChecked(tree->isSymmetric(row));
        box->setToolTip(tooltip);
        QObject::connect(box, &QCheckBox::toggled, tree, [tree, row](bool checked) {
            tree->setSymmetric(row, checked);
        });
        return box;
    }
}

SettingsView::SettingsView(SettingsModel* model, MainTreeModel* tree) {
    addHeading("Symmetry");
    addProperty("Profile", makeSymmetryCheckBox(tree, TopLevelItem::PROFILE_LOWER,
        "Mirror the upper-limb profile onto the lower limb. When checked, the lower-limb profile is hidden and kept in sync with the upper-limb profile."));
    addProperty("Width", makeSymmetryCheckBox(tree, TopLevelItem::WIDTH_LOWER,
        "Mirror the upper-limb width onto the lower limb. When checked, the lower-limb width is hidden and kept in sync with the upper-limb width."));
    addProperty("Layers", makeSymmetryCheckBox(tree, TopLevelItem::LAYERS_LOWER,
        "Mirror the upper-limb layers onto the lower limb. When checked, the lower-limb layers are hidden and kept in sync with the upper-limb layers."));

    addHeading("General");
    addProperty("Limb elements", new IntegerView(model, model->N_LIMB_ELEMENTS, IntegerRange::positive(), Tooltips::SettingsNumLimbElements));
    addProperty("Limb eval points", new IntegerView(model, model->N_EVAL_POINTS, IntegerRange::greaterOrEqual(2), Tooltips::SettingsNumEvalPoints));

    addHeading("Statics");
    addProperty("Min. draw resolution", new IntegerView(model, model->MIN_DRAW_RESOLUTION, IntegerRange::positive(), Tooltips::SettingsMinDrawResolution));
    addProperty("Max. draw resolution", new IntegerView(model, model->MAX_DRAW_RESOLUTION, IntegerRange::positive(), Tooltips::SettingsMaxDrawResolution));
    addProperty("Iteration tolerance", new DoubleView(model, model->STATIC_ITERATION_TOLERANCE, Quantities::none, DoubleRange::positive(1e-6), Tooltips::SettingsStaticIterationTolerance));

    addHeading("Dynamics");
    addProperty("Arrow clamp force", new DoubleView(model, model->ARROW_CLAMP_FORCE, Quantities::force, DoubleRange::nonNegative(0.1), Tooltips::SettingsArrowClampForce));
    addProperty("String compression factor", new DoubleView(model, model->STRING_COMPRESSION_FACTOR, Quantities::ratio, DoubleRange::positive(1e-6), Tooltips::SettingsStringCompressionFactor));
    addProperty("Timespan factor", new DoubleView(model, model->TIMESPAN_FACTOR, Quantities::ratio, DoubleRange::positive(1e-2), Tooltips::SettingsTimespanFactor));
    addProperty("Timeout factor", new DoubleView(model, model->TIMEOUT_FACTOR, Quantities::ratio, DoubleRange::positive(0.1), Tooltips::SettingsTimeoutFactor));
    addProperty("Min. timestep", new DoubleView(model, model->MIN_TIMESTEP, Quantities::time, DoubleRange::positive(1e-6), Tooltips::SettingsMinTimestep));
    addProperty("Max. timestep", new DoubleView(model, model->MAX_TIMESTEP, Quantities::time, DoubleRange::positive(1e-4), Tooltips::SettingsMaxTimestep));
    addProperty("Steps per period", new IntegerView(model, model->STEPS_PER_PERIOD, IntegerRange::positive(), Tooltips::SettingsStepsPerPeriod));
    addProperty("Iteration tolerance", new DoubleView(model, model->DYNAMIC_ITERATION_TOLERANCE, Quantities::none, DoubleRange::positive(1e-6), Tooltips::SettingsDynamicIterationTolerance));

    addStretch();
}
