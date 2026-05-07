#include "MassesView.hpp"
#include "ArrowMassView.hpp"
#include "pre/models/MassesModel.hpp"
#include "primitive/DoubleView.hpp"
#include "pre/utils/DoubleRange.hpp"
#include "pre/models/units/UnitSystem.hpp"
#include "pre/Language.hpp"

MassesView::MassesView(MassesModel* model) {
    addProperty("Arrow", new ArrowMassView(model, model->ARROW));
    addProperty("Limb tip (upper)",   new DoubleView(model, model->LIMB_TIP_UPPER,   Quantities::mass, DoubleRange::nonNegative(1e-3), Tooltips::MassLimbTip));
    addProperty("Limb tip (lower)",   new DoubleView(model, model->LIMB_TIP_LOWER,   Quantities::mass, DoubleRange::nonNegative(1e-3), Tooltips::MassLimbTip));
    addProperty("String nock",        new DoubleView(model, model->STRING_NOCK,      Quantities::mass, DoubleRange::nonNegative(1e-3), Tooltips::MassStringCenter));
    addProperty("String tip (upper)", new DoubleView(model, model->STRING_TIP_UPPER, Quantities::mass, DoubleRange::nonNegative(1e-3), Tooltips::MassStringTip));
    addProperty("String tip (lower)", new DoubleView(model, model->STRING_TIP_LOWER, Quantities::mass, DoubleRange::nonNegative(1e-3), Tooltips::MassStringTip));
    addStretch();
}
