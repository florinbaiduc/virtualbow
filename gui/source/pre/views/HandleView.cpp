#include "HandleView.hpp"
#include "pre/utils/DoubleRange.hpp"
#include "pre/widgets/DoubleSpinBox.hpp"
#include "pre/models/HandleModel.hpp"
#include "pre/models/units/UnitSystem.hpp"
#include "pre/Language.hpp"
#include <QComboBox>
#include <QLabel>

HandleView::HandleView(HandleModel* model) {
    //addProperty("Brace height", new DoubleView(model, model->BRACE_HEIGHT, Quantities::length, DoubleRange::positive(1e-3), Tooltips::BraceHeight));
    //addProperty("Draw length", new DrawLengthView(model, model->DRAW_LENGTH));

    auto selectionBox = new QComboBox();
    selectionBox->addItems({"Flexible", "Rigid"});
    selectionBox->setToolTip(Tooltips::HandleTypeDefinition);
    selectionBox->setItemData(0, Tooltips::HandleTypeFlexible, Qt::ToolTipRole);
    selectionBox->setItemData(1, Tooltips::HandleTypeRigid, Qt::ToolTipRole);

    auto lengthUpperEdit = new DoubleSpinBox(Quantities::length, DoubleRange::nonNegative(1e-3));
    lengthUpperEdit->setToolTip(Tooltips::HandleLength);
    lengthUpperEdit->setValue(0.0);

    auto lengthLowerEdit = new DoubleSpinBox(Quantities::length, DoubleRange::nonNegative(1e-3));
    lengthLowerEdit->setToolTip(Tooltips::HandleLength);
    lengthLowerEdit->setValue(0.0);

    // Angle may be zero (straight join) or negative (deflex) per the tooltip,
    // so use an unrestricted range instead of strictly positive.
    auto angleEdit = new DoubleSpinBox(Quantities::angle, DoubleRange::unrestricted(1e-2));
    angleEdit->setToolTip(Tooltips::HandleAngle);
    angleEdit->setValue(0.0);

    // Pivot may be zero or negative (deflex) per the tooltip; allow any value.
    auto pivotEdit = new DoubleSpinBox(Quantities::length, DoubleRange::unrestricted(1e-3));
    pivotEdit->setToolTip(Tooltips::HandlePivot);
    pivotEdit->setValue(0.0);

    addProperty("Type", selectionBox);
    int lengthUpperRow = addProperty("Length (upper)", lengthUpperEdit);
    int lengthLowerRow = addProperty("Length (lower)", lengthLowerEdit);
    int angleRow = addProperty("Angle", angleEdit);
    int pivotRow = addProperty("Pivot", pivotEdit);
    addStretch();

    // Sets visibility of the widgets according to selection
    auto updateVisibility = [=]() {
        bool visible = selectionBox->currentIndex() != 0;
        setVisible(lengthUpperRow, visible);
        setVisible(lengthLowerRow, visible);
        setVisible(angleRow, visible);
        setVisible(pivotRow, visible);
    };

    // Update view according to the data in the model
    auto updateView = [=]() {
        // Set selection according to the type of mass definition
        Handle handle = model->data(model->HANDLE, Qt::DisplayRole).value<Handle>();
        selectionBox->setCurrentIndex(handle.index());

        if(auto value = std::get_if<FlexibleHandle>(&handle)) {
            // Do nothing
        }
        else if(auto value = std::get_if<RigidHandle>(&handle)) {
            lengthUpperEdit->setValue(value->length_upper);
            lengthLowerEdit->setValue(value->length_lower);
            angleEdit->setValue(value->angle);
            pivotEdit->setValue(value->pivot);
        }
        else {
            throw std::invalid_argument("Unknown variant");
        }

        updateVisibility();
    };

    // Update model according to data in the view
    auto updateModel = [=]() {
        switch(selectionBox->currentIndex()) {
        case 0:
            model->setData(model->HANDLE, QVariant::fromValue<Handle>(FlexibleHandle{}));
            break;

        case 1:
            model->setData(model->HANDLE, QVariant::fromValue<Handle>(RigidHandle{
                .length_upper = lengthUpperEdit->value(),
                .length_lower = lengthLowerEdit->value(),
                .angle = angleEdit->value(),
                .pivot = pivotEdit->value()
            }));
            break;

        default:
            throw std::invalid_argument("Unknown variant");
        }
    };

    // Show the correct editors according to the selected definition
    QObject::connect(selectionBox, &QComboBox::currentIndexChanged, this, updateVisibility);

    // Initialize view from model once
    updateView();

    // Keep model up to date on changes
    QObject::connect(selectionBox, &QComboBox::currentIndexChanged, this, updateModel);
    QObject::connect(lengthUpperEdit, &DoubleSpinBox::valueChanged, this, updateModel);
    QObject::connect(lengthLowerEdit, &DoubleSpinBox::valueChanged, this, updateModel);
    QObject::connect(angleEdit, &DoubleSpinBox::valueChanged, this, updateModel);
    QObject::connect(pivotEdit, &DoubleSpinBox::valueChanged, this, updateModel);
}
