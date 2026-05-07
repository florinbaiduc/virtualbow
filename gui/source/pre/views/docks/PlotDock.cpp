#include "PlotDock.hpp"
#include "pre/models/MainModel.hpp"
#include "pre/models/MainTreeModel.hpp"
#include "pre/views/limb2d/WidthPlotView.hpp"
#include "pre/views/limb2d/HeightPlotView.hpp"
#include "pre/views/limb2d/ProfilePlotView.hpp"
#include <QItemSelectionModel>
#include <QLabel>

class PlaceholderLabel: public QLabel {
public:
    PlaceholderLabel() {
        setObjectName("PlaceholderLabel");
        setStyleSheet("#PlaceholderLabel { background-image:url(:/icons/background.png); background-position: center; background-repeat: no-repeat; }");
    }

    QSize sizeHint() const override {
        return {256, 256};    // Size of the background image. Only relevant on first launch, before the dock was resized by the user.
    }
};

PlotDock::PlotDock(MainModel* model) {
    placeholder = new PlaceholderLabel();

    this->setWindowTitle("Graph");
    this->setObjectName("PlotDock2");    // Required to save state of main window // TODO: Remove 2 after one release cycle?
    this->setFeatures(QDockWidget::NoDockWidgetFeatures);
    this->setWidget(placeholder);

    auto selectionModel = model->getModelTreeSelectionModel();
    QObject::connect(selectionModel, &QItemSelectionModel::selectionChanged, this, [=, this] {
        QModelIndexList selection = selectionModel->selectedIndexes();
        if(selection.size() == 1) {
            QPersistentModelIndex index(selection.first());

            if(index.internalId() == ItemType::LAYER_UPPER || index.internalId() == ItemType::LAYER_LOWER) {
                LimbSide side = MainTreeModel::sideForLayerType((int)index.internalId());
                showPlot(index, [=]{ return new HeightPlotView(model, side, index); });
                return;
            }

            if(index.internalId() == ItemType::TOPLEVEL &&
               (index.row() == TopLevelItem::WIDTH_UPPER || index.row() == TopLevelItem::WIDTH_LOWER)) {
                LimbSide side = (index.row() == TopLevelItem::WIDTH_LOWER) ? LimbSide::Lower : LimbSide::Upper;
                showPlot(index, [=]{ return new WidthPlotView(model, side); });
                return;
            }

            if((index.internalId() == ItemType::TOPLEVEL &&
                (index.row() == TopLevelItem::PROFILE_UPPER || index.row() == TopLevelItem::PROFILE_LOWER)) ||
               index.internalId() == ItemType::SEGMENT_UPPER || index.internalId() == ItemType::SEGMENT_LOWER) {
                LimbSide side;
                if(index.internalId() == ItemType::TOPLEVEL) {
                    side = (index.row() == TopLevelItem::PROFILE_LOWER) ? LimbSide::Lower : LimbSide::Upper;
                } else {
                    side = MainTreeModel::sideForSegmentType((int)index.internalId());
                }
                showPlot(index, [=]{ return new ProfilePlotView(model, side); });
                return;
            }
        }

        showPlaceholder();
    });

    showPlaceholder();
}

void PlotDock::showPlaceholder() {
    setWidget(placeholder);
}

void PlotDock::showPlot(QPersistentModelIndex index, const std::function<QWidget*()>& create) {
    // Remove invalid model indices and delete their associated plots (for example after model reset)
    plots.removeIf([](std::pair<QPersistentModelIndex, QWidget*> pair) {
        if(!pair.first.isValid()) {
            pair.second->deleteLater();
            return true;
        }
        else {
            return false;
        }
    });

    // Check if a plot for the model index exists, create a new one if not
    if(!plots.contains(index)) {
        plots.insert(index, create());
    }

    // Show plot for model index
    setWidget(plots[index]);
}
