#pragma once
#include "pre/models/MainModel.hpp"
#include "solver/BowModel.hpp"
#include "solver/Defaults.hpp"
#include <QAbstractItemModel>
#include <QStringList>

// Types of items in the model tree, exposed via the id's of the model indices.
// v5: layers and profile-segments split into upper/lower variants so the index
// alone identifies which limb a child item belongs to.
enum ItemType {
    TOPLEVEL,
    MATERIAL,           // Single material under "Materials" (shared across limbs)
    LAYER_UPPER,        // Single layer under "Layers (upper)"
    LAYER_LOWER,        // Single layer under "Layers (lower)"
    SEGMENT_UPPER,      // Single profile segment under "Profile (upper)"
    SEGMENT_LOWER       // Single profile segment under "Profile (lower)"
};

// The top-level items in the model tree and their row indices
enum TopLevelItem {
    COMMENTS = 0,
    SETTINGS = 1,
    HANDLE = 2,
    DRAW = 3,
    MATERIALS = 4,
    LAYERS_UPPER = 5,
    LAYERS_LOWER = 6,
    PROFILE_UPPER = 7,
    PROFILE_LOWER = 8,
    WIDTH_UPPER = 9,
    WIDTH_LOWER = 10,
    STRING = 11,
    MASSES = 12,
    DAMPING = 13,

    LAST = DAMPING,
    COUNT = LAST + 1
};

class MainTreeModel: public QAbstractItemModel {
    Q_OBJECT

public:
    MainTreeModel();
    void setBowModel(BowModel* bow);

    bool canInsertMaterial(const QModelIndexList& indexes);
    void insertMaterial(int row);
    void appendMaterial();

    bool canInsertLayer(const QModelIndexList& indexes);
    void insertLayer(LimbSide side, int row);
    void appendLayer(LimbSide side);

    bool canInsertSegment(const QModelIndexList& indexes);
    void insertSegment(LimbSide side, int row, SegmentType type);
    void appendSegment(LimbSide side, SegmentType type);

    bool canRemoveIndexes(QModelIndexList& indexes);
    void removeIndexes(QModelIndexList indexes);
    void removeMaterial(int row);
    void removeLayer(LimbSide side, int row);
    void removeSegment(LimbSide side, int row);

    bool canMoveIndexesUp(const QModelIndexList& indexes);
    void moveIndexesUp(QModelIndexList indexes);

    bool canMoveIndexesDown(const QModelIndexList& indexes);
    void moveIndexesDown(QModelIndexList indexes);

    void swapMaterials(int i, int j);
    void swapLayers(LimbSide side, int i, int j);
    void swapSegments(LimbSide side, int i, int j);

    // Helper: which TopLevelItem hosts items of the given ItemType
    static int parentRowFor(int itemType);
    // Helper: which LimbSide does this layer/segment ItemType belong to
    static LimbSide sideForLayerType(int itemType);
    static LimbSide sideForSegmentType(int itemType);
    // Helper: which LimbSide does a top-level row belong to (for LAYERS_*/PROFILE_*/WIDTH_*)
    static LimbSide sideForTopLevel(int row);

    // Resolve the BowModel's per-limb lists by side
    LimbSection& sectionFor(LimbSide side);
    std::list<Layer>& layersFor(LimbSide side) { return sectionFor(side).layers; }
    std::list<ProfileSegment>& profileFor(LimbSide side);

    // Abstract method implementations

    QModelIndex index(int row, int column, const QModelIndex &parent = QModelIndex()) const override;

    QModelIndex parent(const QModelIndex &index) const override;

    int rowCount(const QModelIndex &parent = QModelIndex()) const override;

    int columnCount(const QModelIndex &parent = QModelIndex()) const override;

    Qt::ItemFlags flags(const QModelIndex &index) const override;

    QVariant data(const QModelIndex &index, int role = Qt::DisplayRole) const override;

    bool setData(const QModelIndex &index, const QVariant &value, int role = Qt::EditRole) override;

signals:
    // Emitted when the tree structure of the bow model has been modified (including names)
    void contentModified();

private:
    BowModel* bow;

    QString topLevelItemName(int row) const;
    QString topLevelToolTip(int row) const;
    QIcon topLevelItemIcon(int row) const;

    QString segmentName(LimbSide side, int row) const;
    QString segmentTooltip(LimbSide side, int row) const;
    QIcon segmentIcon(LimbSide side, int row) const;
};
