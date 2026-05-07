#include "DrawView.hpp"
#include "primitive/DoubleView.hpp"
#include "pre/utils/DoubleRange.hpp"
#include "DrawLengthView.hpp"
#include "pre/models/DrawModel.hpp"
#include "pre/models/units/UnitSystem.hpp"
#include "pre/Language.hpp"

DrawView::DrawView(DrawModel* model) {
    addProperty("Brace height", new DoubleView(model, model->BRACE_HEIGHT, Quantities::length, DoubleRange::positive(1e-3), Tooltips::BraceHeight));
    addProperty("Draw length", new DrawLengthView(model, model->DRAW_LENGTH));
    addProperty("Nock offset", new DoubleView(model, model->NOCK_OFFSET, Quantities::length, DoubleRange::unrestricted(1e-3), "Signed offset of the nocking point along the bow's longitudinal axis (positive = toward upper limb). Zero for symmetric bows; ~+L/6 for a yumi."));
    addStretch();
}
